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
                (self.buffer[$byte] & $bit.bits()) != 0
            }

            pub fn [<set_btn_ $name>](&mut self, value: bool) {
                if value {
                    self.buffer[$byte] += $bit.bits();
                } else {
                    self.buffer[$byte] -= $bit.bits();
                }
            }
        }
    };
}

bitflags! {
struct DS4Buttons: u8 {
    const L2 = 1 << 2;
    const R2 = 1 << 3;
    const TRIANGLE = 1 << 7;
    const CIRCLE = 1 << 6;
    const CROSS = 1 << 5;
    const SQUARE = 1 << 4;
    const DPADUP = 1 << 3;
    const DPADDOWN = 1 << 2;
    const DPADLEFT = 1 << 1;
    const DPADRIGHT = 1 << 0;
    const L1 = 1 << 0;
    const R1 = 1 << 1;
    const SHARE = 1 << 4;
    const OPTIONS = 1 << 5;
    const L3 = 1 << 6;
    const R3 = 1 << 7;
    const PSBUTTON = 1 << 0;
    const TOUCHBUTTON = 1 << 2 - 1;
  }
}

pub struct DS4 {
    buffer: Vec<u8>,
}

#[allow(unused)]
impl DS4 {
    pub fn new(buffer: &[u8]) -> Result<Self, &'static str> {
        if buffer.len() == 64 && buffer[0] == 0x1 {
            Ok(Self {
                buffer: buffer.to_vec(),
            })
        } else {
            Err("not a hid response")
        }
    }

    input_axis!(lx, 1);
    input_axis!(ly, 2);
    input_axis!(rx, 3);
    input_axis!(ry, 4);

    input_button!(triangle, 5, DS4Buttons::TRIANGLE);
    input_button!(circle, 5, DS4Buttons::CIRCLE);
    input_button!(cross, 5, DS4Buttons::CROSS);
    input_button!(square, 5, DS4Buttons::SQUARE);

    input_button!(dpad_up, 5, DS4Buttons::DPADUP);
    input_button!(dpad_down, 5, DS4Buttons::DPADDOWN);
    input_button!(dpad_left, 5, DS4Buttons::DPADLEFT);
    input_button!(dpad_right, 5, DS4Buttons::DPADRIGHT);

    input_button!(l1, 6, DS4Buttons::L1);
    input_button!(r1, 6, DS4Buttons::R1);

    input_axis!(l2, 8);
    input_axis!(r2, 9);
    input_button!(l2_btn, 6, DS4Buttons::L2);
    input_button!(r2_btn, 6, DS4Buttons::R2);

    input_button!(l3, 6, DS4Buttons::L3);
    input_button!(r3, 6, DS4Buttons::R3);

    input_button!(share, 6, DS4Buttons::SHARE);
    input_button!(options, 6, DS4Buttons::OPTIONS);
    input_button!(ps_btn, 7, DS4Buttons::PSBUTTON);
    input_button!(touch_btn, 7, DS4Buttons::TOUCHBUTTON);

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
