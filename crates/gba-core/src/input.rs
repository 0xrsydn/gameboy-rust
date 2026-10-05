//! Platform-independent GBA button state. Bits are pressed-high here;
//! the KEYINPUT register exposes their active-low hardware representation.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Button {
    A = 1 << 0,
    B = 1 << 1,
    Select = 1 << 2,
    Start = 1 << 3,
    Right = 1 << 4,
    Left = 1 << 5,
    Up = 1 << 6,
    Down = 1 << 7,
    R = 1 << 8,
    L = 1 << 9,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Buttons(u16);

impl Buttons {
    pub fn from_bits(bits: u16) -> Self {
        Self(bits & 0x03ff)
    }

    pub fn bits(self) -> u16 {
        self.0
    }

    pub fn with(self, button: Button, pressed: bool) -> Self {
        Self(if pressed {
            self.0 | button as u16
        } else {
            self.0 & !(button as u16)
        })
    }

    pub(crate) fn keyinput(self) -> u16 {
        !self.0 & 0x03ff
    }
}
