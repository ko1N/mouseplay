mod console;
mod controller;
mod hooks;
mod input;

#[macro_use]
extern crate bitflags;

use winapi::um::winnt::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};

#[no_mangle]
extern "system" fn DllMain(_hinst: *const u8, reason: u32, _reserved: *const u8) -> u32 {
    match reason {
        DLL_PROCESS_ATTACH => {
            std::thread::spawn(|| {
                console::init();
                hooks::setup();
            });
        }
        DLL_PROCESS_DETACH => {
            // ...
        }
        _ => {}
    }
    1
}
