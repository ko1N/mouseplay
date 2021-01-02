macro_rules! input_axis {
    ($name:ident, $byte:expr) => {
        paste::item! {
            pub fn [<axis_ $name>](&self) -> u8 {
                self.buffer[$byte]
            }

            pub fn [<set_axis_ $name>](&mut self, value: u8) {
                self.buffer[$byte] = value;
            }
        }
    };
}

macro_rules! input_button {
    ($name:ident, $byte:expr, $bit:expr) => {
        paste::item! {
            pub fn [<btn_ $name>](&self) -> bool {
                (self.buffer[$byte] & $bit) != 0
            }

            pub fn [<set_btn_ $name>](&mut self, down: bool) {
                if down {
                    self.buffer[$byte] |= $bit;
                } else {
                    self.buffer[$byte] &= !$bit;
                }
            }
        }
    };
}

const TRIANGLE: u8 = 1 << 7;
const CIRCLE: u8 = 1 << 6;
const CROSS: u8 = 1 << 5;
const SQUARE: u8 = 1 << 4;

const L1: u8 = 1 << 0;
const R1: u8 = 1 << 1;

const L2: u8 = 1 << 2;
const R2: u8 = 1 << 3;

const L3: u8 = 1 << 6;
const R3: u8 = 1 << 7;

const SHARE: u8 = 1 << 4;
const OPTIONS: u8 = 1 << 5;
const PSBUTTON: u8 = 1 << 0;
const TOUCHBUTTON: u8 = 1 << 2 - 1;

pub struct DS4 {
    buffer: Vec<u8>,
}

#[allow(unused)]
impl DS4 {
    pub fn new(buffer: &[u8]) -> Result<Self, &'static str> {
        if buffer.len() == 64 {
            // && buffer[0] == 0x1 {
            Ok(Self {
                buffer: buffer.to_vec(),
            })
        } else {
            Err("not a hid response")
        }
    }

    pub fn to_raw(&self) -> Vec<u8> {
        self.buffer.clone()
    }

    input_axis!(lx, 1);
    input_axis!(ly, 2);
    input_axis!(rx, 3);
    input_axis!(ry, 4);

    input_button!(triangle, 5, TRIANGLE);
    input_button!(circle, 5, CIRCLE);
    input_button!(cross, 5, CROSS);
    input_button!(square, 5, SQUARE);

    // dpad is in hat format, 0x8 is released state

    input_button!(l1, 6, L1);
    input_button!(r1, 6, R1);

    input_axis!(l2, 8);
    input_axis!(r2, 9);
    input_button!(l2, 6, L2);
    input_button!(r2, 6, R2);

    input_button!(l3, 6, L3);
    input_button!(r3, 6, R3);

    input_button!(share, 6, SHARE);
    input_button!(options, 6, OPTIONS);
    input_button!(ps, 7, PSBUTTON);
    input_button!(touch, 7, TOUCHBUTTON);

    pub fn set_btn(&mut self, button: &str, down: bool) {
        match button {
            "triangle" => self.set_btn_triangle(down),
            "circle" => self.set_btn_circle(down),
            "cross" => self.set_btn_cross(down),
            "square" => self.set_btn_square(down),

            "l1" => self.set_btn_l1(down),
            "r1" => self.set_btn_r1(down),

            "l2" => self.set_btn_l2(down),
            "r2" => self.set_btn_r2(down),

            "l3" => self.set_btn_l3(down),
            "r3" => self.set_btn_r3(down),

            "share" => self.set_btn_share(down),
            "options" => self.set_btn_options(down),
            "ps" => self.set_btn_ps(down),
            "touch" => self.set_btn_touch(down),

            _ => (),
        }
    }

    pub fn set_axis(&mut self, axis: &str, value: u8) {
        match axis {
            "lx" => self.set_axis_lx(value),
            "ly" => self.set_axis_ly(value),
            "rx" => self.set_axis_rx(value),
            "ry" => self.set_axis_ry(value),

            "l2" => self.set_axis_l2(value),
            "r2" => self.set_axis_r2(value),

            _ => (),
        }
    }

    pub fn frame_count(&self) -> u8 {
        self.buffer[7] >> 2
    }

    pub fn battery(&self) -> u8 {
        (self.buffer[30] & 0xF) * 10
    }

    pub fn is_charging(&self) -> bool {
        (self.buffer[30] & 0x10) != 0
    }
}
