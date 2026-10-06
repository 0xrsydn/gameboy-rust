//! Disconnected normal serial shifter with nominal internal clocks.
use super::replace_byte;

pub const SIODATA32: u32 = 0x0400_0120;
pub const SIOCNT: u32 = 0x0400_0128;
pub const SIODATA8: u32 = 0x0400_012a;
pub const RCNT: u32 = 0x0400_0134;
pub const JOYCNT: u32 = 0x0400_0140;
pub const JOY_RECV: u32 = 0x0400_0150;
pub const JOY_TRANS: u32 = 0x0400_0154;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Serial {
    data: u32,
    send: u8,
    control: u16,
    rcnt: u16,
    bits_left: u8,
    until_bit: u32,
}

impl Serial {
    pub(crate) fn mapped(address: u32) -> bool {
        matches!(address,
            SIODATA32..=0x0400_0123 | SIOCNT..=0x0400_012b |
            RCNT..=0x0400_0135 | JOYCNT..=0x0400_0141 |
            JOY_RECV..=0x0400_0157)
    }

    pub(crate) fn unsupported(address: u32, value: u8) -> Option<&'static str> {
        match address {
            0x0400_0129 if value & 0x20 != 0 => Some("multiplayer/UART serial mode"),
            0x0400_0135 if value & 0xc0 == 0xc0 => Some("Joybus serial mode"),
            0x0400_0135 if value & 0x81 == 0x81 => Some("GPIO serial interrupt enable"),
            JOYCNT if value & 0x40 != 0 => Some("Joybus interrupt enable"),
            JOY_RECV..=0x0400_0157 if value != 0 => Some("Joybus data access beyond reset"),
            _ => None,
        }
    }

    fn normal(&self) -> bool {
        self.rcnt & 0x8000 == 0
    }
    fn busy(&self) -> bool {
        self.control & 0x80 != 0
    }
    fn clocked(&self) -> bool {
        self.normal() && self.busy() && self.control & 1 != 0
    }
    fn width(&self) -> u8 {
        if self.control & 0x1000 != 0 {
            32
        } else {
            8
        }
    }
    fn period(&self) -> u32 {
        if self.control & 2 != 0 {
            8
        } else {
            64
        }
    }

    /// Validate the final access value at its bus-completion phase, not a transient byte state.
    pub(crate) fn validate(&self, address: u32, bytes: &[u8]) -> Option<(u32, u8, &'static str)> {
        for (offset, &value) in bytes.iter().enumerate() {
            let at = address + offset as u32;
            if let Some(operation) = Self::unsupported(at, value) {
                return Some((at, value, operation));
            }
        }
        let mut next = *self;
        next.merge(address, bytes);
        // Clearing start always permits cancellation/reconfiguration. Other
        // live changes after a shifted bit lack an independently verified model.
        if self.normal() && self.busy() && next.busy() {
            let changed_mode = self.normal() != next.normal();
            let changed_clock = (self.control ^ next.control) & 0x1003 != 0;
            let data_touched = (0..bytes.len()).any(|offset| {
                let at = address + offset as u32;
                if self.width() == 32 {
                    (SIODATA32..SIODATA32 + 4).contains(&at)
                } else {
                    at == SIODATA8
                }
            });
            if (self.clocked() && changed_mode)
                || (self.bits_left != self.width()
                    && (changed_clock || changed_mode || data_touched))
            {
                return Some((
                    address,
                    bytes[0],
                    "serial reconfiguration during active transfer",
                ));
            }
        }
        None
    }

    fn gpio_pins(&self) -> u16 {
        let outputs = (self.rcnt >> 4) & 15;
        (self.rcnt & outputs & 15) | (!outputs & 15)
    }

    pub(crate) fn read8(&self, address: u32) -> Option<u8> {
        if (SIODATA32..SIODATA32 + 4).contains(&address) {
            return Some(self.data.to_le_bytes()[(address - SIODATA32) as usize]);
        }
        let value = match address & !1 {
            SIOCNT => {
                self.control
                    | if self.normal() {
                        4
                    } else {
                        self.gpio_pins() & 4
                    }
            }
            SIODATA8 => u16::from(self.send),
            RCNT => {
                if self.normal() && address & 1 == 0 {
                    return None;
                }
                (self.rcnt & !15) | self.gpio_pins()
            }
            JOYCNT | JOY_RECV..=0x0400_0156 => 0,
            _ => return None,
        };
        Some(value.to_le_bytes()[(address & 1) as usize])
    }

    fn merge(&mut self, address: u32, bytes: &[u8]) {
        for (offset, &value) in bytes.iter().enumerate() {
            let at = address + offset as u32;
            if (SIODATA32..SIODATA32 + 4).contains(&at) {
                let shift = (at - SIODATA32) * 8;
                self.data = (self.data & !(255 << shift)) | (u32::from(value) << shift);
            } else {
                match at & !1 {
                    SIOCNT => self.control = replace_byte(self.control, at, value) & 0x508b,
                    SIODATA8 if at & 1 == 0 => self.send = value,
                    RCNT => self.rcnt = replace_byte(self.rcnt, at, value) & 0xc1ff,
                    _ => {}
                }
            }
        }
    }

    pub(crate) fn write(&mut self, address: u32, bytes: &[u8]) {
        debug_assert!(self.validate(address, bytes).is_none());
        let previous = *self;
        self.merge(address, bytes);
        if !self.normal() || !self.busy() {
            self.bits_left = 0;
            self.until_bit = 0;
        } else if !previous.normal()
            || !previous.busy()
            || (previous.control ^ self.control) & 0x1003 != 0
        {
            // New request or an unshifted clock/width change, including external -> internal.
            self.bits_left = self.width();
            self.until_bit = self.period();
        }
    }

    pub(crate) fn next_event_cycles(&self) -> Option<u32> {
        self.clocked()
            .then(|| self.until_bit + u32::from(self.bits_left - 1) * self.period())
    }

    /// At most 32 edges per request. SI is pulled high without a connected partner.
    pub(crate) fn advance(&mut self, mut cycles: u32) -> u16 {
        if !self.clocked() {
            return 0;
        }
        while cycles >= self.until_bit {
            cycles -= self.until_bit;
            if self.width() == 32 {
                self.data = (self.data << 1) | 1;
            } else {
                self.send = (self.send << 1) | 1;
            }
            self.bits_left -= 1;
            if self.bits_left == 0 {
                self.control &= !0x80;
                self.until_bit = 0;
                return if self.control & 0x4000 != 0 { 0x80 } else { 0 };
            }
            self.until_bit = self.period();
        }
        self.until_bit -= cycles;
        0
    }
}
