use crate::controller::ds4::DS4;

use std::ffi::OsString;
use std::ffi::{c_void, CStr, CString};
use std::mem::size_of;

use log::info;

use libc::strcmp;
use winapi::{
    shared::minwindef::LPCVOID,
    um::{
        handleapi::INVALID_HANDLE_VALUE,
        libloaderapi::GetModuleHandleA,
        memoryapi::VirtualProtect,
        minwinbase::LPSECURITY_ATTRIBUTES,
        winnt::{
            LPCWSTR, PAGE_EXECUTE_READWRITE, PIMAGE_DOS_HEADER, PIMAGE_IMPORT_BY_NAME,
            PIMAGE_IMPORT_DESCRIPTOR, PIMAGE_NT_HEADERS, PIMAGE_THUNK_DATA,
        },
    },
};
use winapi::{
    shared::{
        minwindef::{BOOL, DWORD, LPDWORD, LPVOID},
        ntdef::HANDLE,
    },
    um::minwinbase::LPOVERLAPPED,
};

static CONTROLLER_FILE_HANDLE_SPOOF: u64 = 1234;

// TODO: fix potential thread safety issue (RwLock)
static mut CONTROLLER_FILE_HANDLE: HANDLE = std::ptr::null_mut();

unsafe fn hook_import(
    target_module: &str,
    import_module: &str,
    import_name: &str,
    hook_func: *mut c_void,
) -> Result<*mut c_void, &'static str> {
    let target_module_cstr =
        CString::new(target_module).map_err(|_| "unable to convert target_module to CString")?;
    let target_module_base = GetModuleHandleA(target_module_cstr.as_ptr()) as u32;
    if target_module_base == 0 {
        return Err("unable to find target module");
    }

    let p_dos_header = target_module_base as PIMAGE_DOS_HEADER;
    if (*p_dos_header).e_magic != 0x5A4D
    /* 'MZ' */
    {
        return Err("invalid dos header e_magic value");
    }

    let p_nt_headers = (target_module_base + (*p_dos_header).e_lfanew as u32) as PIMAGE_NT_HEADERS;
    if (*p_nt_headers).Signature != 0x00004550
    /* 'PE00' */
    {
        return Err("invalid nt header signature");
    }

    let import_module_cstr =
        CString::new(import_module).map_err(|_| "unable to convert import_module to CString")?;
    let mut p_import_descriptor = (target_module_base
        + (*p_nt_headers).OptionalHeader.DataDirectory[1].VirtualAddress as u32)
        as PIMAGE_IMPORT_DESCRIPTOR; // PEIMAGE_DIRECTORY_ENTRY_IMPORT
    while *(*p_import_descriptor).u.OriginalFirstThunk() != 0 {
        if strcmp(
            import_module_cstr.as_ptr(),
            (target_module_base + (*p_import_descriptor).Name as u32) as *const i8,
        ) == 0
        {
            // iterate funcs
            let mut p_thunk = (target_module_base + (*p_import_descriptor).FirstThunk as u32)
                as PIMAGE_THUNK_DATA;
            let mut p_orig_thunk = (target_module_base
                + *(*p_import_descriptor).u.OriginalFirstThunk() as u32)
                as PIMAGE_THUNK_DATA;
            while *(*p_thunk).u1.AddressOfData() != 0 {
                // check func name
                let thunk_import = (target_module_base + *(*p_orig_thunk).u1.AddressOfData() as u32)
                    as PIMAGE_IMPORT_BY_NAME;
                if let Ok(thunk_import_name_str) =
                    CStr::from_ptr((*thunk_import).Name.as_ptr() as *const i8).to_str()
                {
                    if thunk_import_name_str.to_lowercase() == import_name.to_lowercase() {
                        // hook
                        info!(
                            "hooking function {}!{} at 0x{:x}",
                            import_module,
                            import_name,
                            *(*p_thunk).u1.AddressOfData()
                        );

                        let mut old_prot = 0u32;
                        VirtualProtect(
                            p_thunk as _,
                            size_of::<LPVOID>(),
                            PAGE_EXECUTE_READWRITE,
                            &mut old_prot as _,
                        );
                        let orig_func = *(p_thunk as *const u32);
                        *(p_thunk as *mut u32) = hook_func as u32;
                        VirtualProtect(
                            p_thunk as _,
                            size_of::<LPVOID>(),
                            old_prot,
                            &mut old_prot as _,
                        );
                        return Ok(orig_func as *mut c_void);
                    }
                }

                p_thunk = p_thunk.offset(1);
                p_orig_thunk = p_orig_thunk.offset(1);
            }

            return Err("import not found");
        }

        p_import_descriptor = p_import_descriptor.offset(1);
    }

    Err("import module not found")
}

static mut ORIG_IS_DEBUGGER_PRESENT: *const c_void = std::ptr::null_mut();
unsafe extern "stdcall" fn hook_is_debugger_present() -> BOOL {
    0
}

unsafe fn u16_ptr_to_string(ptr: *const u16) -> OsString {
    use std::os::windows::prelude::*;

    let len = (0..).take_while(|&i| *ptr.offset(i) != 0).count();
    let slice = std::slice::from_raw_parts(ptr, len);
    OsString::from_wide(slice)
}

static mut ORIG_CREATE_FILE: *const c_void = std::ptr::null_mut();
unsafe extern "stdcall" fn hook_create_file(
    lp_file_name: LPCWSTR,
    dw_desired_access: DWORD,
    dw_share_mode: DWORD,
    lp_security_attributes: LPSECURITY_ATTRIBUTES,
    dw_creation_disposition: DWORD,
    dw_flags_and_attributes: DWORD,
    h_template_file: HANDLE,
) -> HANDLE {
    //info!("hook_create_file(): lp_file_name={:?}, dw_desired_access={:?}, dw_share_mode={:?}, lp_security_attributes={:?}, dw_creation_disposition={:?}, dw_flags_and_attributes={:?}, h_template_file={:?}",
    //  lp_file_name, dw_desired_access, dw_share_mode, lp_security_attributes, dw_creation_disposition, dw_flags_and_attributes, h_template_file);

    let orig_func: extern "stdcall" fn(
        _: LPCWSTR,
        _: DWORD,
        _: DWORD,
        _: LPSECURITY_ATTRIBUTES,
        _: DWORD,
        _: DWORD,
        _: HANDLE,
    ) -> HANDLE = std::mem::transmute(ORIG_CREATE_FILE);
    let result = orig_func(
        lp_file_name,
        dw_desired_access,
        dw_share_mode,
        lp_security_attributes,
        dw_creation_disposition,
        dw_flags_and_attributes,
        h_template_file,
    );

    let lp_file_name_str = u16_ptr_to_string(lp_file_name);
    let is_controller = lp_file_name_str
        == "\\\\?\\hid#rev_01#6&39fdb758&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";

    if is_controller {
        info!(
            "opening ds4 file handle: {}",
            lp_file_name_str.to_str().unwrap()
        );

        if result == INVALID_HANDLE_VALUE {
            info!("spoofing presence of ds4");
            CONTROLLER_FILE_HANDLE = CONTROLLER_FILE_HANDLE_SPOOF as _;
            return CONTROLLER_FILE_HANDLE_SPOOF as _;
        } else {
            CONTROLLER_FILE_HANDLE = result;
        }
    }

    result
}

static mut ORIG_READ_FILE: *const c_void = std::ptr::null_mut();
unsafe extern "stdcall" fn hook_read_file(
    h_file: HANDLE,
    lp_buffer: LPVOID,
    n_number_of_bytes_to_read: DWORD,
    lp_number_of_bytes_read: LPDWORD,
    lp_overlapped: LPOVERLAPPED,
) -> BOOL {
    //trace!("hook_read_file(): h_file={:?}, lp_bufer={:?}, n_number_of_bytes_to_read={:?}, lp_number_of_bytes_read={:?}, lp_overlapped={:?}",
    //  h_file, lp_buffer, n_number_of_bytes_to_read, lp_number_of_bytes_read, lp_overlapped);

    let mut bytes_read = 0;

    // only call original func if we are not spoofing a controller presence
    let result = if h_file != CONTROLLER_FILE_HANDLE_SPOOF as _ {
        let orig_func: extern "stdcall" fn(
            _: HANDLE,
            _: LPVOID,
            _: DWORD,
            _: LPDWORD,
            _: LPOVERLAPPED,
        ) -> BOOL = std::mem::transmute(ORIG_READ_FILE);
        orig_func(
            h_file,
            lp_buffer,
            n_number_of_bytes_to_read,
            &mut bytes_read as _,
            lp_overlapped,
        )
    } else {
        bytes_read = n_number_of_bytes_to_read;
        1
    };

    if !lp_number_of_bytes_read.is_null() {
        *lp_number_of_bytes_read = bytes_read;
    }

    // TODO:
    crate::input::raw_input::hijack_wndproc().unwrap();

    if h_file == CONTROLLER_FILE_HANDLE {
        println!("reading controller: {}", bytes_read);
    }

    // TODO: figure out if we are in ds4 or ds5 mode
    let buffer = std::slice::from_raw_parts_mut(lp_buffer as *mut u8, bytes_read as usize);
    if let Ok(mut ds4) = DS4::new(buffer) {
        /*
        println!(
            "lx={}, ly={}, rx={}, ry={}, frame_count={}, battery={}, is_charging={}",
            ds4.axis_lx(),
            ds4.axis_ly(),
            ds4.axis_rx(),
            ds4.axis_ry(),
            ds4.frame_count(),
            ds4.battery(),
            ds4.is_charging(),
        );
        */

        if let Ok(mut raw_input) = crate::input::raw_input::RAW_INPUT.write() {
            if let Ok(mut mapper) = crate::mapper::MAPPER.write() {
                if let Some(mapper) = mapper.as_mut() {
                    raw_input.accumulate();

                    mapper.map_controller(&raw_input, &mut ds4);
                    buffer.copy_from_slice(ds4.to_raw().as_slice());
                }
            }
        }
    }

    result
}

static mut ORIG_WRITE_FILE: *const c_void = std::ptr::null_mut();
unsafe extern "stdcall" fn hook_write_file(
    h_file: HANDLE,
    lp_buffer: LPCVOID,
    n_number_of_bytes_to_write: DWORD,
    lp_number_of_bytes_written: LPDWORD,
    lp_overlapped: LPOVERLAPPED,
) -> BOOL {
    //info!("hook_write_file(): h_file={:?}, lp_bufer={:?}, n_number_of_bytes_to_write={:?}, lp_number_of_bytes_written={:?}, lp_overlapped={:?}",
    //  h_file, lp_buffer, n_number_of_bytes_to_write, lp_number_of_bytes_written, lp_overlapped);

    if n_number_of_bytes_to_write == 32 {
        let slice = std::slice::from_raw_parts_mut(
            lp_buffer as *mut u8,
            n_number_of_bytes_to_write as usize,
        );
        //slice[1] = 5;
        slice[4] = 100; // rumble
                        //slice[8] = 5; // rumble time?
    }

    let mut bytes_written = 0;
    let orig_func: extern "stdcall" fn(
        _: HANDLE,
        _: LPCVOID,
        _: DWORD,
        _: LPDWORD,
        _: LPOVERLAPPED,
    ) -> BOOL = std::mem::transmute(ORIG_WRITE_FILE);
    let result = orig_func(
        h_file,
        lp_buffer,
        n_number_of_bytes_to_write,
        &mut bytes_written as _,
        lp_overlapped,
    );
    if !lp_number_of_bytes_written.is_null() {
        *lp_number_of_bytes_written = bytes_written;
    }

    if bytes_written == 32 {
        let slice = std::slice::from_raw_parts(lp_buffer as *mut u8, bytes_written as usize);
        //info!("written: {:?}", slice);
    }

    result
}

pub fn setup() {
    // hook:
    // RpCtrlWrapper.dll", "KERNEL32.dll", "IsDebuggerPresent"
    // RpCtrlWrapper.dll", "KERNEL32.dll", "CreateFileW"
    // RpCtrlWrapper.dll", "KERNEL32.dll", "ReadFile"
    // RpCtrlWrapper.dll", "KERNEL32.dll", "WriteFile"

    unsafe {
        ORIG_IS_DEBUGGER_PRESENT = hook_import(
            "RpCtrlWrapper.dll",
            "KERNEL32.dll",
            "IsDebuggerPresent",
            hook_is_debugger_present as _,
        )
        .unwrap();

        ORIG_CREATE_FILE = hook_import(
            "RpCtrlWrapper.dll",
            "KERNEL32.dll",
            "CreateFileW",
            hook_create_file as _,
        )
        .unwrap();

        ORIG_READ_FILE = hook_import(
            "RpCtrlWrapper.dll",
            "KERNEL32.dll",
            "ReadFile",
            hook_read_file as _,
        )
        .unwrap();
    }

    info!("hooking kernel32.dll!WriteFile");
    unsafe {
        ORIG_WRITE_FILE = hook_import(
            "RpCtrlWrapper.dll",
            "KERNEL32.dll",
            "WriteFile",
            hook_write_file as _,
        )
        .unwrap();
    }
}
