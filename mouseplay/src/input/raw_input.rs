use std::collections::HashMap;
use std::ffi::{c_void, CString};
use std::mem::size_of;
use std::sync::RwLock;

use log::{info, warn};

use lazy_static::lazy_static;
use winapi::{
    shared::{
        minwindef::{LOWORD, LPARAM, LRESULT, UINT, WPARAM},
        windef::{HWND, RECT},
        windowsx::GET_Y_LPARAM,
    },
    um::winuser::{
        CallWindowProcW, FindWindowA, GetRawInputData, GetWindowLongPtrA, GetWindowRect,
        RegisterRawInputDevices, SetCapture, SetCursor, SetCursorPos, SetWindowLongPtrA,
        ShowCursor, GWL_WNDPROC, HTCLIENT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT,
        RIM_TYPEKEYBOARD, RIM_TYPEMOUSE, VK_ESCAPE, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
        VK_XBUTTON1, VK_XBUTTON2, WM_INPUT, WM_KEYDOWN, WM_PARENTNOTIFY, WNDPROC,
    },
};

// thread safe storage for all known wndprocs
lazy_static! {
    static ref ORIG_WNDPROCS: RwLock<HashMap<u64, u64>> = RwLock::new(HashMap::new());
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

    /*
      case WM_LBUTTONDOWN:
      case WM_LBUTTONDBLCLK:
      case WM_LBUTTONUP:
      case WM_RBUTTONDOWN:
      case WM_RBUTTONDBLCLK:
      case WM_RBUTTONUP:
      case WM_MBUTTONDOWN:
      case WM_MBUTTONDBLCLK:
      case WM_MBUTTONUP:
        return TRUE;

      case WM_KEYDOWN:
      case WM_SYSKEYDOWN:
      case WM_KEYUP:
      case WM_SYSKEYUP:
        return TRUE;
    */

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

bitflags! {
struct MouseButtons: u32 {
    const LBUTTONDOWN = 1 << 0;
    const LBUTTONUP = 1 << 1;
    const RBUTTONDOWN = 1 << 2;
    const RBUTTONUP = 1 << 3;
    const MBUTTONDOWN = 1 << 4;
    const MBUTTONUP = 1 << 5;
    const XBUTTON1DOWN = 1 << 6;
    const XBUTTON1UP = 1 << 7;
    const XBUTTON2DOWN = 1 << 8;
    const XBUTTON2UP = 1 << 9;
  }
}

pub struct RawInput {
    capture: Option<u64>,
    mouse_lock: bool,
    keys: [bool; 256],
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
            })
        } else {
            Err("unable to register raw input devices")
        }
    }

    pub fn parse(
        &mut self,
        h_wnd: HWND,
        u_msg: UINT,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> Result<bool, &'static str> {
        match u_msg {
            WM_INPUT => {
                // we do not always get WM_INPUT messages so we wait until the connection is established
                self.capture = Some(h_wnd as u64);
                if self.mouse_lock {
                    //SetCapture(h_wnd);
                    self.parse_raw_input(l_param)?;
                    //self.center_mouse(h_wnd);
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
                        if LOWORD(l_param as _) as isize == HTCLIENT {
                            println!("removing cursor!!!");
                            unsafe { SetCursor(std::ptr::null_mut()) };
                        }
                        return Ok(false);
                    } else {
                    }
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
                // TODO: compute delta + accumulate mouse input

                //this->m_mouse[0] += raw->data.mouse.lLastX;
                //this->m_mouse[1] += raw->data.mouse.lLastY;

                let mouse_x = mouse.lLastX;
                let mouse_y = mouse.lLastY;

                //println!("mouse_x={}, mouse_y={}", mouse_x, mouse_y);

                if mouse.ulRawButtons & (MouseButtons::LBUTTONDOWN.bits() as u32) != 0 {
                    self.keys[VK_LBUTTON as usize] = true;
                } else if mouse.ulRawButtons & (MouseButtons::LBUTTONUP.bits() as u32) != 0 {
                    self.keys[VK_LBUTTON as usize] = false;
                } else if mouse.ulRawButtons & (MouseButtons::RBUTTONDOWN.bits() as u32) != 0 {
                    self.keys[VK_RBUTTON as usize] = true;
                } else if mouse.ulRawButtons & (MouseButtons::RBUTTONUP.bits() as u32) != 0 {
                    self.keys[VK_RBUTTON as usize] = false;
                } else if mouse.ulRawButtons & (MouseButtons::MBUTTONDOWN.bits() as u32) != 0 {
                    self.keys[VK_MBUTTON as usize] = true;
                } else if mouse.ulRawButtons & (MouseButtons::MBUTTONUP.bits() as u32) != 0 {
                    self.keys[VK_MBUTTON as usize] = false;
                } else if mouse.ulRawButtons & (MouseButtons::XBUTTON1DOWN.bits() as u32) != 0 {
                    self.keys[VK_XBUTTON1 as usize] = true;
                } else if mouse.ulRawButtons & (MouseButtons::XBUTTON1UP.bits() as u32) != 0 {
                    self.keys[VK_XBUTTON1 as usize] = false;
                } else if mouse.ulRawButtons & (MouseButtons::XBUTTON2DOWN.bits() as u32) != 0 {
                    self.keys[VK_XBUTTON2 as usize] = true;
                } else if mouse.ulRawButtons & (MouseButtons::XBUTTON2UP.bits() as u32) != 0 {
                    self.keys[VK_XBUTTON2 as usize] = false;
                }
            }
            RIM_TYPEKEYBOARD => {
                let keyboard = unsafe { (*rid).data.keyboard() };
                if keyboard.Flags == 0 {
                    if keyboard.VKey as i32 == VK_ESCAPE {
                        info!("unlocking mouse");
                        self.mouse_lock = false;
                    }

                    // down
                    println!("key down: {}", keyboard.VKey);
                    self.keys[keyboard.VKey as usize] = true;
                } else {
                    // up
                    println!("key up: {}", keyboard.VKey);
                    self.keys[keyboard.VKey as usize] = false;
                }
            }
            _ => {}
        };

        Ok(())
    }

    // TODO: find better name for this
    pub fn translate(&mut self) {
        if let Some(h_wnd) = self.capture {
            // TODO: handle self.mouse_lock == false

            // after each translation we re-center the mouse
            if self.mouse_lock {
                self.center_mouse(h_wnd as HWND);
            //unsafe { ShowCursor(0) };
            } else {
                // TODO: only once
                //unsafe { ShowCursor(1) };
            }
        }
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
