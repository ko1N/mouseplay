use std::ffi::CString;
use std::fs::OpenOptions;
use std::os::windows::io::AsRawHandle;

use log::{info, Level};

use winapi::um::consoleapi::AllocConsole;
use winapi::um::processenv::SetStdHandle;
use winapi::um::winbase::{STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};
use winapi::um::wincon::SetConsoleTitleA;

pub fn init() {
    if unsafe { AllocConsole() } != 0 {
        // console title
        let console_title = CString::new("mouseplay").unwrap();
        unsafe { SetConsoleTitleA(console_title.as_ptr()) };

        // output redirection
        let file = OpenOptions::new()
            .write(true)
            .read(true)
            .open("CONOUT$")
            .unwrap();
        unsafe {
            SetStdHandle(
                STD_OUTPUT_HANDLE,
                file.as_raw_handle() as *mut winapi::ctypes::c_void,
            );
            SetStdHandle(
                STD_ERROR_HANDLE,
                file.as_raw_handle() as *mut winapi::ctypes::c_void,
            );
        }
        std::mem::forget(file);

        // setup logging
        simple_logger::SimpleLogger::new()
            .with_level(Level::Debug.to_level_filter())
            .init()
            .unwrap();

        // print header
        println!(
            "                                             __           
      ____ ___  ____  __  __________  ____  / /___ ___  __
     / __ `__ \\/ __ \\/ / / / ___/ _ \\/ __ \\/ / __ `/ / / /
    / / / / / / /_/ / /_/ (__  )  __/ /_/ / / /_/ / /_/ / 
   /_/ /_/ /_/\\____/\\__,_/____/\\___/ .___/_/\\__,_/\\__, /  
                                  /_/            /____/"
        );
        println!("");
        info!("console initialized");
    }
}
