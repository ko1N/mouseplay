use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use log::info;

use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use winapi::{
    shared::minwindef::MAX_PATH,
    um::{libloaderapi::GetModuleFileNameA, winnt::IMAGE_DOS_HEADER},
};

extern "C" {
    pub static __ImageBase: u8;
}

lazy_static! {
    // thread safe storage for the global Mapper
    pub static ref MAPPER: RwLock<Option<Mapper>> = RwLock::new(None);
}

pub fn load(file_name: &str) -> Result<(), &'static str> {
    let library_dir = get_library_dir()?;
    let mut lock = MAPPER.write().map_err(|_| "unable to lock mapper")?;
    *lock = Some(Mapper::load(library_dir.join(file_name))?);
    Ok(())
}

fn get_library_dir() -> Result<PathBuf, &'static str> {
    let mut buffer = vec![0u8; MAX_PATH];
    unsafe {
        GetModuleFileNameA(
            &__ImageBase as *const _ as _,
            buffer.as_mut_ptr() as _,
            MAX_PATH as u32,
        )
    };
    if let Some((n, _)) = buffer.iter().enumerate().find(|(_, c)| **c == 0_u8) {
        buffer.truncate(n);
    }
    let file_name = PathBuf::from(String::from_utf8_lossy(&buffer).to_string());
    let file_path = file_name
        .parent()
        .ok_or("unable to get library parent directory")?;
    info!("library dir: {:?}", file_path);
    Ok(file_path.to_path_buf())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Mapping {
    Button {
        input: String,
        output: String,
    },
    Axis {
        input: String,
        output: String,
        output_value: f32,
    },
    Mouse {
        output: String,
        multiplier_x: f64,
        multiplier_y: f64,
        exponent: f64,
        dead_zone_x: i32,
        dead_zone_y: i32,
        shape: String,
    },
}

pub struct Mapper {
    mappings: Vec<Mapping>,
}

impl Mapper {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, &'static str> {
        let contents = std::fs::read_to_string(path).map_err(|_| "unable to open mappings file")?;
        let mappings: Vec<Mapping> =
            serde_json::from_str(&contents).map_err(|_| "unable to parse mappings file")?;
        Ok(Self { mappings })
    }
}
