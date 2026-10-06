//! Disconnected serial: normal shifting and idle multiplayer child configuration.
use super::replace_byte;

pub const SIODATA32: u32 = 0x0400_0120;
pub const SIOMULTI0: u32 = SIODATA32;
pub const SIOMULTI1: u32 = 0x0400_0122;
pub const SIOMULTI2: u32 = 0x0400_0124;
pub const SIOMULTI3: u32 = 0x0400_0126;
pub const SIOCNT: u32 = 0x0400_0128;
pub const SIODATA8: u32 = 0x0400_012a;
pub const SIOMLT_SEND: u32 = SIODATA8;
pub const RCNT: u32 = 0x0400_0134;
pub const JOYCNT: u32 = 0x0400_0140;
pub const JOY_RECV: u32 = 0x0400_0150;
pub const JOY_TRANS: u32 = 0x0400_0154;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Serial {
    data: u32,        // Shared SIODATA32 / SIOMULTI0-1 lanes.
    multi_extra: u32, // SIOMULTI2-3; no connected parent can update these.
    send: u16,        // Shared low byte; high byte is accessible only in multiplayer format.
    control: u16,
    rcnt: u16,
    bits_left: u8,
    until_bit: u32,
}

impl Serial {
    pub(crate) fn mapped(address: u32) -> bool {
        matches!(address,
            SIODATA32..=0x0400_012b |
            RCNT..=0x0400_0135 | JOYCNT..=0x0400_0141 |
            JOY_RECV..=0x0400_0157)
    }

    pub(crate) fn unsupported(address: u32, value: u8) -> Option<&'static str> {
        match address {
            0x0400_0129 if value & 0x30 == 0x30 => Some("UART serial mode"),
            0x0400_0135 if value & 0xc0 == 0xc0 => Some("Joybus serial mode"),
            0x0400_0135 if value & 0x81 == 0x81 => Some("GPIO serial interrupt enable"),
            JOYCNT if value & 0x40 != 0 => Some("Joybus interrupt enable"),
            JOY_RECV..=0x0400_0157 if value != 0 => Some("Joybus data access beyond reset"),
            _ => None,
        }
    }

    fn gpio(&self) -> bool {
        self.rcnt & 0x8000 != 0
    }
    fn multiplayer_format(&self) -> bool {
        self.control & 0x3000 == 0x2000
    }
    fn normal(&self) -> bool {
        !self.gpio() && !self.multiplayer_format()
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
        if !next.multiplayer_format()
            && (0..bytes.len())
                .any(|offset| (SIOMULTI2..SIOCNT).contains(&(address + offset as u32)))
        {
            return Some((
                address,
                bytes[0],
                "multiplayer receive registers outside multiplayer mode",
            ));
        }
        // Clearing start always permits cancellation/reconfiguration. Other
        // live changes after a shifted bit lack an independently verified model.
        // Check the written start bit before mode-specific read-only masking.
        // A high-byte mode change must not silently cancel a clocked normal transfer.
        if self.normal() && self.busy() && self.control_value(address, bytes) & 0x80 != 0 {
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
        if (SIOMULTI2..SIOCNT).contains(&address) {
            return self
                .multiplayer_format()
                .then(|| self.multi_extra.to_le_bytes()[(address - SIOMULTI2) as usize]);
        }
        let value = match address & !1 {
            SIOCNT => {
                let pins = if self.gpio() { self.gpio_pins() } else { 15 };
                // SI is pulled high without a cable. Multiplayer drives SD high while idle.
                // ID is a deterministic zero placeholder until a transfer (none can occur here).
                self.control
                    | (pins & 4)
                    | if self.multiplayer_format() {
                        (pins & 2) << 2
                    } else {
                        0
                    }
            }
            SIODATA8 => {
                if self.multiplayer_format() {
                    self.send
                } else {
                    self.send & 255
                }
            }
            RCNT => {
                if !self.gpio() && address & 1 == 0 {
                    return None;
                }
                (self.rcnt & !15) | self.gpio_pins()
            }
            JOYCNT | JOY_RECV..=0x0400_0156 => 0,
            _ => return None,
        };
        Some(value.to_le_bytes()[(address & 1) as usize])
    }

    fn control_value(&self, address: u32, bytes: &[u8]) -> u16 {
        let mut control = self.control;
        for (offset, &value) in bytes.iter().enumerate() {
            let at = address + offset as u32;
            if (SIOCNT..SIOCNT + 2).contains(&at) {
                control = replace_byte(control, at, value);
            }
        }
        control
    }

    fn merge(&mut self, address: u32, bytes: &[u8]) {
        // Select the final format first: a word can change mode and write SEND together.
        let control = self.control_value(address, bytes);
        self.control = control
            & if control & 0x3000 == 0x2000 {
                0x6f03
            } else {
                0x508b
            };
        for (offset, &value) in bytes.iter().enumerate() {
            let at = address + offset as u32;
            if (SIODATA32..SIODATA32 + 4).contains(&at) {
                let shift = (at - SIODATA32) * 8;
                self.data = (self.data & !(255 << shift)) | (u32::from(value) << shift);
            } else if (SIOMULTI2..SIOCNT).contains(&at) {
                let shift = (at - SIOMULTI2) * 8;
                self.multi_extra =
                    (self.multi_extra & !(255 << shift)) | (u32::from(value) << shift);
            } else {
                match at & !1 {
                    SIODATA8 if self.multiplayer_format() || at & 1 == 0 => {
                        self.send = replace_byte(self.send, at, value);
                    }
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
                self.send = (self.send & 0xff00) | u16::from(((self.send as u8) << 1) | 1);
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
