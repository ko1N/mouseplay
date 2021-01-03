use std::collections::HashMap;
use std::ffi::{c_void, CString};
use std::mem::size_of;
use std::sync::RwLock;

use log::{info, trace, warn};

use lazy_static::lazy_static;
use winapi::{
    shared::{
        minwindef::{LOWORD, LPARAM, LRESULT, UINT, WPARAM},
        windef::{HWND, RECT},
        windowsx::GET_Y_LPARAM,
    },
    um::winuser::*,
};

lazy_static! {
    // thread safe storage for all known wndprocs
    static ref ORIG_WNDPROCS: RwLock<HashMap<u64, u64>> = RwLock::new(HashMap::new());

    // thread safe storage for the global RawInput handler
    pub static ref RAW_INPUT: RwLock<RawInput> = RwLock::new(RawInput::new().unwrap());
}

pub fn hijack_wndproc() -> Result<(), &'static str> {
    let window_name =
        CString::new("PS Remote Play").map_err(|_| "unable to convert window_name")?;
    let h_wnd = unsafe { FindWindowA(std::ptr::null(), window_name.as_ptr()) };
    if h_wnd.is_null() {
        return Err("window not found");
    }

    let orig_wndproc = unsafe { GetWindowLongPtrA(h_wnd, GWL_WNDPROC) } as *const c_void;
    if orig_wndproc == (hook_wndproc as _) {
        // already hijacked, skip
        return Ok(());
    }

    // hijack wndproc
    if let Ok(mut wndprocs) = ORIG_WNDPROCS.write() {
        info!(
            "hijacking wndproc for h_wnd=0x{:x}, orig_wndproc=0x{:x}, hook_wndproc=0x{:x}",
            h_wnd as u64, orig_wndproc as u64, hook_wndproc as *const c_void as u64
        );
        wndprocs.insert(h_wnd as u64, orig_wndproc as u64);
        unsafe { SetWindowLongPtrA(h_wnd, GWL_WNDPROC, hook_wndproc as _) };
    };

    Ok(())
}

unsafe extern "system" fn hook_wndproc(
    h_wnd: HWND,
    u_msg: UINT,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    //info!("hook_wndproc(): h_wnd=0x{:x} u_msg={:x} w_param={:x} l_param={:x}", h_wnd as u64, u_msg, w_param, l_param);

    // TODO: re-center mouse + prevent inputs from being fed into the original window
    let call_wndproc = if let Ok(mut raw_input) = RAW_INPUT.write() {
        raw_input.parse(h_wnd, u_msg, w_param, l_param).unwrap()
    } else {
        true
    };

    if call_wndproc {
        if let Ok(lock) = ORIG_WNDPROCS.read() {
            if let Some(wndproc) = lock.get(&(h_wnd as u64)) {
                if *wndproc != 0 {
                    let orig_func: WNDPROC = Some(std::mem::transmute(*wndproc as *const c_void));
                    return CallWindowProcW(orig_func, h_wnd, u_msg, w_param, l_param);
                }
            } else {
                warn!("wndproc for h_wnd=0x{:x} not found", h_wnd as u64);
            }
        }
    }

    0
}

const LBUTTONDOWN: u16 = 1 << 0;
const LBUTTONUP: u16 = 1 << 1;
const RBUTTONDOWN: u16 = 1 << 2;
const RBUTTONUP: u16 = 1 << 3;
const MBUTTONDOWN: u16 = 1 << 4;
const MBUTTONUP: u16 = 1 << 5;
const XBUTTON1DOWN: u16 = 1 << 6;
const XBUTTON1UP: u16 = 1 << 7;
const XBUTTON2DOWN: u16 = 1 << 8;
const XBUTTON2UP: u16 = 1 << 9;
pub struct RawInput {
    capture: Option<u64>,
    mouse_lock: bool,
    keys: [bool; 256],
    mouse: [i32; 2],
    mouse_accumulator: [i32; 2],
}

impl RawInput {
    pub fn new() -> Result<Self, &'static str> {
        let rid = [
            RAWINPUTDEVICE {
                usUsagePage: 0x01,
                usUsage: 0x02,
                dwFlags: 0,
                hwndTarget: std::ptr::null_mut(),
            },
            RAWINPUTDEVICE {
                usUsagePage: 0x01,
                usUsage: 0x06,
                dwFlags: 0,
                hwndTarget: std::ptr::null_mut(),
            },
        ];

        info!("registering raw input devices");
        if unsafe { RegisterRawInputDevices(rid.as_ptr(), 2, size_of::<RAWINPUTDEVICE>() as u32) }
            != 0
        {
            Ok(Self {
                capture: None,
                mouse_lock: false,
                keys: [false; 256],
                mouse: [0; 2],
                mouse_accumulator: [0; 2],
            })
        } else {
            Err("unable to register raw input devices")
        }
    }

    pub fn key(&self, button: &str) -> bool {
        match str_to_vk(button) {
            Ok(vk) => self.keys[vk as usize],
            Err(_) => false,
        }
    }

    // TODO:
    pub fn mouse_x(&self) -> i32 {
        self.mouse[0]
    }

    pub fn mouse_y(&self) -> i32 {
        self.mouse[1]
    }

    pub fn parse(
        &mut self,
        h_wnd: HWND,
        u_msg: UINT,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> Result<bool, &'static str> {
        // validate existence of capture wnd
        if let Some(h_wnd) = self.capture {
            if unsafe { IsWindow(h_wnd as _) } == 0 {
                self.capture = None;
            }
        }

        match u_msg {
            WM_INPUT => {
                // we do not always get WM_INPUT messages so we wait until the connection is established
                self.capture = Some(h_wnd as u64);
                self.parse_raw_input(l_param)?;
                if self.mouse_lock {
                    return Ok(false);
                }
            }
            WM_PARENTNOTIFY if w_param == 513 => {
                if self.capture.is_some() && !self.mouse_lock {
                    let mut rect = RECT::default();
                    // TODO: check return value for error
                    unsafe { GetWindowRect(h_wnd, &mut rect as _) };
                    let y = GET_Y_LPARAM(l_param);
                    if y <= (rect.bottom - rect.top) - 130 {
                        info!("locking mouse");
                        self.mouse_lock = true;
                        return Ok(false);
                    }
                }
            }
            WM_SETCURSOR => {
                if self.capture.is_some() {
                    if self.mouse_lock {
                        // TODO: remove cursor?
                        /*if LOWORD(l_param as _) as isize == HTCLIENT {
                            unsafe { SetCursor(std::ptr::null_mut()) };
                        }*/
                        return Ok(false);
                    } else {
                    }
                }
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_LBUTTONUP | WM_RBUTTONDOWN
            | WM_RBUTTONDBLCLK | WM_RBUTTONUP | WM_MBUTTONDOWN | WM_MBUTTONDBLCLK
            | WM_MBUTTONUP | WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP => {
                if self.capture.is_some() && self.mouse_lock {
                    return Ok(false);
                }
            }

            _ => {}
        }
        Ok(true)
    }

    fn parse_raw_input(&mut self, l_param: LPARAM) -> Result<(), &'static str> {
        // figure out correct raw_input size
        let mut rid_size = 0u32;
        unsafe {
            GetRawInputData(
                l_param as _,
                RID_INPUT,
                std::ptr::null_mut(),
                &mut rid_size as _,
                size_of::<RAWINPUTHEADER>() as u32,
            )
        };

        let mut data = vec![0u8; rid_size as usize];
        unsafe {
            GetRawInputData(
                l_param as _,
                RID_INPUT,
                data.as_mut_ptr() as _,
                &mut rid_size as _,
                size_of::<RAWINPUTHEADER>() as u32,
            )
        };
        // TODO: check return value size

        let rid = data.as_ptr() as *mut RAWINPUT;
        match unsafe { (*rid).header.dwType } {
            RIM_TYPEMOUSE => {
                let mouse = unsafe { (*rid).data.mouse() };
                self.parse_mouse(mouse);
            }
            RIM_TYPEKEYBOARD => {
                let kbd = unsafe { (*rid).data.keyboard() };
                self.parse_keyboard(kbd);
            }
            _ => {}
        };

        Ok(())
    }

    fn parse_mouse(&mut self, mouse: &RAWMOUSE) {
        self.mouse_accumulator[0] += mouse.lLastX;
        self.mouse_accumulator[1] += mouse.lLastY;

        if mouse.usButtonFlags & LBUTTONDOWN != 0 {
            self.keys[VK_LBUTTON as usize] = true;
        }
        if mouse.usButtonFlags & LBUTTONUP != 0 {
            self.keys[VK_LBUTTON as usize] = false;
        }
        if mouse.usButtonFlags & RBUTTONDOWN != 0 {
            self.keys[VK_RBUTTON as usize] = true;
        }
        if mouse.usButtonFlags & RBUTTONUP != 0 {
            self.keys[VK_RBUTTON as usize] = false;
        }
        if mouse.usButtonFlags & MBUTTONDOWN != 0 {
            self.keys[VK_MBUTTON as usize] = true;
        }
        if mouse.usButtonFlags & MBUTTONUP != 0 {
            self.keys[VK_MBUTTON as usize] = false;
        }
        if mouse.usButtonFlags & XBUTTON1DOWN != 0 {
            self.keys[VK_XBUTTON1 as usize] = true;
        }
        if mouse.usButtonFlags & XBUTTON1UP != 0 {
            self.keys[VK_XBUTTON1 as usize] = false;
        }
        if mouse.usButtonFlags & XBUTTON2DOWN != 0 {
            self.keys[VK_XBUTTON2 as usize] = true;
        }
        if mouse.usButtonFlags & XBUTTON2UP != 0 {
            self.keys[VK_XBUTTON2 as usize] = false;
        }
    }

    fn parse_keyboard(&mut self, kbd: &RAWKEYBOARD) {
        if kbd.Flags == 0 {
            trace!("key up: {}", kbd.VKey);
            self.keys[kbd.VKey as usize] = true;
        } else {
            trace!("key up: {}", kbd.VKey);
            self.keys[kbd.VKey as usize] = false;
        }

        // check unlock key combination shift+escape
        if self.keys[VK_SHIFT as usize] && self.keys[VK_ESCAPE as usize] {
            info!("unlocking mouse");
            self.mouse_lock = false;
        }
    }

    pub fn accumulate(&mut self) {
        if let Some(h_wnd) = self.capture {
            // TODO: handle self.mouse_lock == false

            // unlock mouse
            if self.mouse_lock && unsafe { IsWindow(h_wnd as _) } == 0 {
                self.mouse_lock = false;
            }

            if self.mouse_lock {
                self.mouse[0] = self.mouse_accumulator[0];
                self.mouse[1] = self.mouse_accumulator[1];
                self.center_mouse(h_wnd as HWND);
            } else {
                self.mouse[0] = 0;
                self.mouse[1] = 0;
            }
        }

        // reset mouse
        self.mouse_accumulator[0] = 0;
        self.mouse_accumulator[1] = 0;
    }

    fn center_mouse(&self, h_wnd: HWND) {
        let mut rect = RECT::default();
        // TODO: check return value for error
        unsafe {
            GetWindowRect(h_wnd, &mut rect as _);
            SetCursorPos(
                ((rect.right + rect.left) as f32 / 2.0) as i32,
                ((rect.bottom + rect.top) as f32 / 2.0) as i32,
            );
        }
    }
}

fn str_to_vk(key: &str) -> Result<i32, &'static str> {
    match key {
        "mouse1" => Ok(VK_LBUTTON),
        "mouse2" => Ok(VK_RBUTTON),
        "mouse3" => Ok(VK_MBUTTON),
        "mouse4" => Ok(VK_XBUTTON1),
        "mouse5" => Ok(VK_XBUTTON2),
        "shift" => Ok(VK_SHIFT),
        "lshift" => Ok(VK_LSHIFT),
        "rshift" => Ok(VK_RSHIFT),
        "alt" => Ok(VK_MENU),
        "lalt" => Ok(VK_LMENU),
        "ralt" => Ok(VK_RMENU),
        "ctrl" => Ok(VK_CONTROL),
        "lctrl" => Ok(VK_LCONTROL),
        "rctrl" => Ok(VK_RCONTROL),
        "tab" => Ok(VK_TAB),
        "up" => Ok(VK_UP),
        "down" => Ok(VK_DOWN),
        "left" => Ok(VK_LEFT),
        "right" => Ok(VK_RIGHT),
        "insert" => Ok(VK_INSERT),
        "delete" => Ok(VK_DELETE),
        "home" => Ok(VK_HOME),
        "end" => Ok(VK_END),
        "pgup" => Ok(VK_PRIOR),
        "pgdn" => Ok(VK_NEXT),
        "backspace" => Ok(VK_BACK),
        "enter" => Ok(VK_RETURN),
        "pause" => Ok(VK_PAUSE),
        "numlock" => Ok(VK_NUMLOCK),
        "space" => Ok(VK_SPACE),
        "kp_0" => Ok(VK_NUMPAD0),
        "kp_1" => Ok(VK_NUMPAD1),
        "kp_2" => Ok(VK_NUMPAD2),
        "kp_3" => Ok(VK_NUMPAD3),
        "kp_4" => Ok(VK_NUMPAD4),
        "kp_5" => Ok(VK_NUMPAD5),
        "kp_6" => Ok(VK_NUMPAD6),
        "kp_7" => Ok(VK_NUMPAD7),
        "kp_8" => Ok(VK_NUMPAD8),
        "kp_9" => Ok(VK_NUMPAD9),
        "esc" => Ok(VK_ESCAPE),
        "escape" => Ok(VK_ESCAPE),
        "f1" => Ok(VK_F1),
        "f2" => Ok(VK_F2),
        "f3" => Ok(VK_F3),
        "f4" => Ok(VK_F4),
        "f5" => Ok(VK_F5),
        "f6" => Ok(VK_F6),
        "f7" => Ok(VK_F7),
        "f8" => Ok(VK_F8),
        "f9" => Ok(VK_F9),
        "f10" => Ok(VK_F10),
        "f11" => Ok(VK_F11),
        "f12" => Ok(VK_F12),
        "0" => Ok(58),
        "1" => Ok(49),
        "2" => Ok(50),
        "3" => Ok(51),
        "4" => Ok(52),
        "5" => Ok(53),
        "6" => Ok(54),
        "7" => Ok(55),
        "8" => Ok(56),
        "9" => Ok(57),
        "a" => Ok(65),
        "b" => Ok(66),
        "c" => Ok(67),
        "d" => Ok(68),
        "e" => Ok(69),
        "f" => Ok(70),
        "g" => Ok(71),
        "h" => Ok(72),
        "i" => Ok(73),
        "j" => Ok(74),
        "k" => Ok(75),
        "l" => Ok(76),
        "m" => Ok(77),
        "n" => Ok(78),
        "o" => Ok(79),
        "p" => Ok(80),
        "q" => Ok(81),
        "r" => Ok(82),
        "s" => Ok(83),
        "t" => Ok(84),
        "u" => Ok(85),
        "v" => Ok(86),
        "w" => Ok(87),
        "x" => Ok(88),
        "y" => Ok(89),
        "z" => Ok(90),
        _ => Err("invalid keycode"),
    }
}
