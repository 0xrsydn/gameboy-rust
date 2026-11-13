//! Disconnected serial: normal shifting, idle multiplayer, and register-level UART, GPIO, and
//! Joybus. Register reads follow the mGBA test suite's hardware results for every mode.
use super::replace_byte;
use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};

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
pub const JOYSTAT: u32 = 0x0400_0158;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal8,
    Normal32,
    Multiplayer,
    Uart,
    Gpio,
    Joybus,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Serial {
    data: u32, // SIODATA32, shared with SIOMULTI0-1. Readable in normal 32-bit and multiplayer.
    send: u16, // SIODATA8 / SIOMLT_SEND. Normal transfers shift only the low byte.
    control: u16,
    rcnt: u16,
    joy_control: u8,   // IRQ enable only; no external master can set event flags.
    joy_transmit: u32, // Latched for a master; CPU reads return zero.
    joy_status: u8,    // Software flags and transmit pending; no remote reception.
    bits_left: u8,
    until_bit: u32,
}

/// Retained SIOCNT bits for each control format; the mode bits are included.
fn control_mask(control: u16) -> u16 {
    match control & 0x3000 {
        0x2000 => 0x6f83,
        0x3000 => 0x7f8f,
        _ => 0x5f8b,
    }
}

impl Serial {
    pub(crate) fn mapped(address: u32) -> bool {
        matches!(address,
            SIODATA32..=0x0400_012b |
            RCNT..=0x0400_0135 | JOYCNT..=0x0400_0141 |
            JOY_RECV..=0x0400_0159)
    }

    fn gpio(&self) -> bool {
        self.rcnt & 0xc000 == 0x8000
    }
    fn multiplayer_format(&self) -> bool {
        self.control & 0x3000 == 0x2000
    }
    fn uart_format(&self) -> bool {
        self.control & 0x3000 == 0x3000
    }
    fn mode(&self) -> Mode {
        match (self.rcnt & 0xc000, self.control & 0x3000) {
            (0x8000, _) => Mode::Gpio,
            (0xc000, _) => Mode::Joybus,
            (_, 0x0000) => Mode::Normal8,
            (_, 0x1000) => Mode::Normal32,
            (_, 0x2000) => Mode::Multiplayer,
            _ => Mode::Uart,
        }
    }
    fn normal(&self) -> bool {
        matches!(self.mode(), Mode::Normal8 | Mode::Normal32)
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
        let mut next = *self;
        next.merge(address, bytes);
        let touches = |range: std::ops::Range<u32>| {
            (0..bytes.len() as u32)
                .find(|&offset| range.contains(&(address + offset)))
                .map(|offset| (address + offset, bytes[offset as usize]))
        };
        // SI falling edges request GPIO interrupts; their timing is unverified.
        if next.gpio() && next.rcnt & 0x100 != 0 && next.gpio_pins() & 4 == 0 {
            let (at, value) = touches(RCNT..RCNT + 2).unwrap_or((address, bytes[0]));
            return Some((at, value, "GPIO serial interrupt with SI low"));
        }
        // A UART data write with sending enabled would transmit to no partner.
        if next.mode() == Mode::Uart && next.control & 0x400 != 0 {
            if let Some((at, value)) = touches(SIODATA8..SIODATA8 + 2) {
                return Some((at, value, "UART transmission"));
            }
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

    /// Pins SC, SD, SI, and SO as RCNT bits 0-3 read them without a cable. Normal mode drives
    /// SO from SIOCNT bit 3 while idle; its transfer waveform is not modeled.
    fn pins(&self) -> Option<u16> {
        Some(match self.mode() {
            Mode::Gpio => self.gpio_pins(),
            Mode::Joybus => 0xc,
            Mode::Multiplayer | Mode::Uart => 0xf,
            Mode::Normal8 | Mode::Normal32 if self.busy() => return None,
            Mode::Normal8 | Mode::Normal32 => 0x5 | (self.control & 8),
        })
    }

    pub(crate) fn read8(&self, address: u32) -> Option<u8> {
        let mode = self.mode();
        if (SIODATA32..SIODATA32 + 4).contains(&address) {
            let readable = matches!(mode, Mode::Normal32 | Mode::Multiplayer);
            let data = if readable { self.data } else { 0 };
            return Some(data.to_le_bytes()[(address - SIODATA32) as usize]);
        }
        // No partner fills SIOMULTI2-3, JOY_RECV, or reads back JOY_TRANS.
        if (SIOMULTI2..SIOCNT).contains(&address) || (JOY_RECV..JOYSTAT).contains(&address) {
            return Some(0);
        }
        let value = match address & !1 {
            SIOCNT => {
                let pins = if self.gpio() { self.gpio_pins() } else { 15 };
                if self.uart_format() {
                    // Send buffer not full; receive buffer empty.
                    self.control | 0x20
                } else if self.multiplayer_format() {
                    // SI is pulled high without a cable. Multiplayer drives SD high while idle.
                    // ID is a deterministic zero placeholder until a transfer (none can occur here).
                    self.control | (pins & 4) | (pins & 2) << 2
                } else {
                    self.control | (pins & 4)
                }
            }
            SIODATA8 if mode == Mode::Uart => 0, // Empty receive buffer.
            SIODATA8 => self.send,
            RCNT => {
                let pins = if address & 1 == 0 { self.pins()? } else { 0 };
                (self.rcnt & !15) | pins
            }
            JOYCNT => u16::from(self.joy_control),
            JOYSTAT => u16::from(self.joy_status),
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
        self.control = control & control_mask(control);
        let mode = self.mode();
        for (offset, &value) in bytes.iter().enumerate() {
            let at = address + offset as u32;
            if (SIODATA32..SIODATA32 + 4).contains(&at) {
                // Multiplayer receive registers are written only by transfers.
                if mode != Mode::Multiplayer {
                    let shift = (at - SIODATA32) * 8;
                    self.data = (self.data & !(255 << shift)) | (u32::from(value) << shift);
                }
            } else if (SIOMULTI2..SIOCNT).contains(&at) || (JOY_RECV..JOY_TRANS).contains(&at) {
                // Receive registers; CPU writes are ignored.
            } else if (JOY_TRANS..JOYSTAT).contains(&at) {
                let shift = (at - JOY_TRANS) * 8;
                self.joy_transmit =
                    (self.joy_transmit & !(255 << shift)) | (u32::from(value) << shift);
                self.joy_status |= 2; // Data available, not transmission complete.
            } else {
                match at & !1 {
                    JOYCNT if at & 1 == 0 => self.joy_control = value & 0x40,
                    JOYSTAT if at & 1 == 0 => {
                        self.joy_status = (self.joy_status & 2) | (value & 0x30);
                    }
                    // A UART data write fills the transmit buffer (diagnostic when sending).
                    SIODATA8 if mode != Mode::Uart => {
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

impl Snapshot for Serial {
    fn save(&self, writer: &mut Writer) {
        let Self {
            data,
            send,
            control,
            rcnt,
            joy_control,
            joy_transmit,
            joy_status,
            bits_left,
            until_bit,
        } = self;
        writer.tag(b"SER1");
        writer.u32(*data);
        writer.u16(*send);
        writer.u16(*control);
        writer.u16(*rcnt);
        writer.u8(*joy_control);
        writer.u32(*joy_transmit);
        writer.u8(*joy_status);
        writer.u8(*bits_left);
        writer.u32(*until_bit);
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self {
            data,
            send,
            control,
            rcnt,
            joy_control,
            joy_transmit,
            joy_status,
            bits_left,
            until_bit,
        } = self;
        reader.tag(b"SER1")?;
        *data = reader.u32()?;
        *send = reader.u16()?;
        *control = reader.u16()?;
        *rcnt = reader.u16()?;
        *joy_control = reader.u8()?;
        *joy_transmit = reader.u32()?;
        *joy_status = reader.u8()?;
        *bits_left = reader.u8()?;
        *until_bit = reader.u32()?;

        if *control & !control_mask(*control) != 0 {
            return Err(SnapshotError::Corrupt("serial control"));
        }
        if *rcnt & !0xc1ff != 0 || *joy_control & !0x40 != 0 || *joy_status & !0x32 != 0 {
            return Err(SnapshotError::Corrupt("serial register"));
        }

        let transfer = *rcnt & 0x8000 == 0 && *control & 0x2000 == 0 && *control & 0x80 != 0;
        if transfer {
            let width = if *control & 0x1000 != 0 { 32 } else { 8 };
            let period = if *control & 2 != 0 { 8 } else { 64 };
            if *bits_left == 0 || *bits_left > width || *until_bit == 0 || *until_bit > period {
                return Err(SnapshotError::Corrupt("serial transfer"));
            }
        } else if *bits_left != 0 || *until_bit != 0 {
            return Err(SnapshotError::Corrupt("serial transfer"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    fn state() -> Serial {
        Serial {
            data: 0x1234_5678,
            send: 0x5aa5,
            control: 0x1083,
            rcnt: 0x0123,
            joy_control: 0x40,
            joy_transmit: 0xfeed_face,
            joy_status: 0x32,
            bits_left: 23,
            until_bit: 4,
        }
    }

    fn encode(serial: &Serial) -> Vec<u8> {
        let mut writer = Writer::new();
        serial.save(&mut writer);
        writer.finish().unwrap()
    }

    #[test]
    fn round_trip_and_reject_every_strict_prefix() {
        let source = state();
        let bytes = encode(&source);
        let mut restored = Serial::default();
        let mut reader = Reader::new(&bytes);
        restored.load(&mut reader).unwrap();
        reader.finish().unwrap();
        assert_eq!(restored, source);

        for end in 0..bytes.len() {
            let mut truncated = Reader::new(&bytes[..end]);
            assert!(
                Serial::default().load(&mut truncated).is_err(),
                "accepted prefix of length {end}"
            );
        }
    }

    #[test]
    fn invalid_control_bits_are_corrupt() {
        let mut bytes = encode(&state());
        bytes[14..16].copy_from_slice(&u16::MAX.to_le_bytes());
        let result = Serial::default().load(&mut Reader::new(&bytes));
        assert!(matches!(result, Err(SnapshotError::Corrupt(_))));
    }
}
