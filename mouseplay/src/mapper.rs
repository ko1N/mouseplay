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

use crate::{controller::ds4::DS4, input::raw_input::RawInput};

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
pub struct ButtonMapping {
    input: String,
    output: String,
}

impl ButtonMapping {
    fn map_controller(&mut self, raw_input: &RawInput, ds4: &mut DS4) {
        let down = raw_input.key(&self.input);
        if down {
            ds4.set_btn(&self.output, down);
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AxisMapping {
    input: String,
    output: String,
    value: f32,
}

impl AxisMapping {
    fn map_controller(&mut self, raw_input: &RawInput, ds4: &mut DS4) {
        let down = raw_input.key(&self.input);
        if down {
            let value = (0.5f32 + (self.value.max(-1f32).min(1f32) / 2f32)) * 255f32;
            //trace!("axis={}, value={}", self.output, value);
            ds4.set_axis(&self.output, value as u8);
        }
    }
}

const FREQUENCY_SCALE: f64 = 2.8125;

#[derive(Debug, Serialize, Deserialize)]
pub struct MouseMapping {
    output_x: String,
    output_y: String,
    multiplier_x: f64,
    multiplier_y: f64,
    dead_zone_x: i32,
    dead_zone_y: i32,

    sensitivity: f64,
    exponent: f64,
    shape: String,

    // internal state
    #[serde(skip)]
    remainder: [i32; 2],
    #[serde(skip)]
    residue: [f64; 2],
    #[serde(skip)]
    axis_hist: [Vec<i32>; 2],
}

impl MouseMapping {
    fn map_controller(&mut self, raw_input: &RawInput, ds4: &mut DS4) {
        // this is roughly a copy of the implementation of gimx
        let mut mouse = [
            raw_input.mouse_x() as f64 * self.sensitivity,
            raw_input.mouse_y() as f64 * self.sensitivity,
        ];

        if mouse[0] != 0f64 || mouse[1] != 0f64 {
            mouse[0] += self.remainder[0] as f64;
            mouse[1] += self.remainder[1] as f64;
        } else {
            self.residue[0] = 0f64;
            self.residue[1] = 0f64;
            return;
        }

        let mut axis = [0i32; 2];

        let max_axis = 128;
        let min_axis = -128;

        let axis_scale = 1f64;

        let multiplier = [
            self.multiplier_x * axis_scale,
            self.multiplier_y * axis_scale,
        ];

        let exponent = self.exponent;

        let mut dead_zone = [
            f64::copysign(
                self.dead_zone_x as f64 * axis_scale,
                self.multiplier_x * mouse[0],
            ),
            f64::copysign(
                self.dead_zone_y as f64 * axis_scale,
                self.multiplier_y * mouse[1],
            ),
        ];

        let hypotenuse = f64::hypot(mouse[0], mouse[1]);
        let angle_cos = mouse[0].abs() / hypotenuse;
        let angle_sin = mouse[1].abs() / hypotenuse;

        if mouse[0] != 0f64 && mouse[1] != 0f64 && self.shape == "circle" {
            dead_zone[0] *= angle_cos;
            dead_zone[1] *= angle_sin;
        }

        let norm = hypotenuse * FREQUENCY_SCALE;

        let z = norm.powf(exponent);

        let z_x = multiplier[0] * f64::copysign(z * angle_cos, mouse[0]);
        let z_y = multiplier[0] * f64::copysign(z * angle_sin, mouse[1]);

        let raw_output = [
            Self::update_axis(&mut axis[0], dead_zone[0], z_x, max_axis, min_axis),
            Self::update_axis(&mut axis[1], dead_zone[1], z_y, max_axis, min_axis),
        ];

        self.remainder[0] = Self::update_controller_axis(
            ds4,
            &self.output_x,
            &mut axis[0],
            &mut self.axis_hist[0],
            min_axis,
        );
        self.remainder[1] = Self::update_controller_axis(
            ds4,
            &self.output_y,
            &mut axis[1],
            &mut self.axis_hist[1],
            min_axis,
        );

        self.update_residue(mouse, axis, axis_scale, multiplier, raw_output);
    }

    fn update_axis(axis: &mut i32, dead_zone: f64, z: f64, max_axis: i32, min_axis: i32) -> f64 {
        let mut raw = z;

        if z.abs() >= 1f64 {
            raw = z + dead_zone;
        }

        *axis = raw as i32;

        if *axis < min_axis || *axis > max_axis {
            raw = *axis as f64;
        }

        raw
    }

    fn update_controller_axis(
        ds4: &mut DS4,
        output: &str,
        axis: &mut i32,
        axis_hist: &mut Vec<i32>,
        min_axis: i32,
    ) -> i32 {
        let mut remainder = 0;

        *axis -= min_axis;
        if *axis > 255 {
            remainder = *axis - 255;
            *axis = 255;
        } else if *axis < 0 {
            remainder = *axis;
            *axis = 0;
        }

        // update controller state
        let _axis = *axis;
        ds4.set_axis(output, _axis as u8);

        axis_hist.push(_axis);
        if axis_hist.len() > 256 {
            axis_hist.remove(0);
        }

        return remainder;
    }

    fn update_residue(
        &mut self,
        mouse: [f64; 2],
        axis: [i32; 2],
        axis_scale: f64,
        multiplier: [f64; 2],
        output_raw: [f64; 2],
    ) {
        let mut input_trunk = [0f64; 2];

        if axis[0] != 0 || axis[1] != 0 {
            let zx = axis[0].abs() as f64;
            let mut zy = axis[1].abs() as f64;

            let mut dead_zone = [
                self.dead_zone_x as f64 * axis_scale,
                self.dead_zone_y as f64 * axis_scale,
            ];

            if zx == 0f64 {
                zy -= dead_zone[1];
                zy = zy.max(0f64);
                input_trunk[1] = f64::copysign(
                    (zy / (multiplier[1].abs() * FREQUENCY_SCALE.powf(self.exponent)))
                        .powf(1f64 / self.exponent),
                    multiplier[1] * output_raw[1],
                );
            } else if zy == 0f64 {
                /*
                 * approximate the residue vector angle:
                 *
                 *   theta = gamma * alpha / beta
                 *
                 * with:
                 *
                 *   alpha: input motion angle
                 *   beta: desired output motion angle
                 *   gamma: truncated output motion angle
                 *   theta: truncated input motion angle
                 */
                let angle = (zy / zx).atan() * (mouse[1].abs() / mouse[0].abs()).atan()
                    / (output_raw[1].abs() / output_raw[0].abs()).atan();
                let angle_cos = angle.cos();
                let angle_sin = angle.sin();

                if self.shape == "circle" {
                    dead_zone[0] *= angle_cos;
                    dead_zone[1] *= angle_sin;
                }

                let normx = ((zx - dead_zone[0])
                    / (multiplier[0].abs() * FREQUENCY_SCALE.powf(self.exponent) * angle_cos))
                    .powf(1.0 / self.exponent);
                let normy = ((zy - dead_zone[1])
                    / (multiplier[1].abs() * FREQUENCY_SCALE.powf(self.exponent) * angle_sin))
                    .powf(1.0 / self.exponent);
                input_trunk[0] = f64::copysign(angle_cos * normx, multiplier[0] * output_raw[0]);
                input_trunk[1] = f64::copysign(angle_sin * normy, multiplier[1] * output_raw[1]);
            }
        }

        self.residue[0] = if input_trunk[0] != 0f64 {
            mouse[0] - input_trunk[0]
        } else {
            0f64
        };
        self.residue[1] = if input_trunk[1] != 0f64 {
            mouse[1] - input_trunk[1]
        } else {
            0f64
        };
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Mapping {
    Button(ButtonMapping),
    Axis(AxisMapping),
    Mouse(MouseMapping),
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

    pub fn map_controller(&mut self, raw_input: &RawInput, ds4: &mut DS4) {
        for mapping in self.mappings.iter_mut() {
            match mapping {
                Mapping::Button(mapping) => {
                    mapping.map_controller(raw_input, ds4);
                }
                Mapping::Axis(mapping) => {
                    mapping.map_controller(raw_input, ds4);
                }
                Mapping::Mouse(mapping) => {
                    mapping.map_controller(raw_input, ds4);
                }
            }
        }
    }
}
