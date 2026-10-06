//! Disabled sound and disconnected serial initialization state.
//! Sound enable, FIFO writes, serial transfers, UART/multiplayer/Joybus modes,
//! and GPIO interrupts remain explicit diagnostics, not silent device stubs.

use super::replace_byte;

pub const SOUND_START: u32 = 0x0400_0060;
pub const SOUNDCNT_H: u32 = 0x0400_0082;
pub const SOUNDCNT_X: u32 = 0x0400_0084;
pub const SOUNDBIAS: u32 = 0x0400_0088;
pub const WAVE_RAM: u32 = 0x0400_0090;
pub const SIODATA32: u32 = 0x0400_0120;
pub const SIOCNT: u32 = 0x0400_0128;
pub const SIODATA8: u32 = 0x0400_012a;
pub const RCNT: u32 = 0x0400_0134;
pub const JOYCNT: u32 = 0x0400_0140;
pub const JOY_RECV: u32 = 0x0400_0150;
pub const JOY_TRANS: u32 = 0x0400_0154;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Inactive {
    sound_control: u16,
    bias: u16,
    // With sound disabled, SOUND3CNT_L is zero. The CPU accesses bank 1;
    // bank 0 stays zero because enabling sound/bank selection is unsupported.
    wave: [u8; 16],
    serial_data: [u8; 4],
    serial_send: u8,
    serial_control: u16,
    rcnt: u16,
}

impl Inactive {
    pub(super) fn mapped(address: u32) -> bool {
        matches!(address,
            SOUND_START..=0x0400_008b | WAVE_RAM..=0x0400_009f |
            SIODATA32..=0x0400_0123 | SIOCNT..=0x0400_012b |
            RCNT..=0x0400_0135 | JOYCNT..=0x0400_0141 |
            JOY_RECV..=0x0400_0157)
    }

    /// Value-dependent rejection is independent of device state, so batch
    /// stores can validate all values before committing any RAM/I/O writes.
    pub(super) fn unsupported(address: u32, value: u8) -> Option<&'static str> {
        match address {
            SOUNDCNT_X if value & 0x80 != 0 => {
                Some("sound master enable (audio engine not implemented)")
            }
            SIOCNT if value & 0x80 != 0 => Some("serial transfer start"),
            0x0400_0129 if value & 0x20 != 0 => Some("multiplayer/UART serial mode"),
            0x0400_0135 if value & 0x40 != 0 => Some("Joybus serial mode"),
            0x0400_0135 if value & 1 != 0 => Some("GPIO serial interrupt enable"),
            JOYCNT if value & 0x40 != 0 => Some("Joybus interrupt enable"),
            JOY_RECV..=0x0400_0157 if value != 0 => Some("Joybus data access beyond reset"),
            _ => None,
        }
    }

    fn gpio_pins(&self) -> u16 {
        let outputs = (self.rcnt >> 4) & 15;
        (self.rcnt & outputs & 15) | (!outputs & 15) // Disconnected inputs have pull-ups.
    }

    pub(super) fn read8(&self, address: u32) -> Option<u8> {
        if (WAVE_RAM..=WAVE_RAM + 15).contains(&address) {
            return Some(self.wave[(address - WAVE_RAM) as usize]);
        }
        if (SIODATA32..=SIODATA32 + 3).contains(&address) {
            return Some(self.serial_data[(address - SIODATA32) as usize]);
        }
        let value = match address & !1 {
            SOUNDCNT_H => self.sound_control,
            SOUNDBIAS => self.bias,
            SOUND_START..=0x0400_008a => 0, // PSG stays reset while master enable is clear.
            SIOCNT => {
                let si = if self.rcnt & 0x8000 != 0 {
                    self.gpio_pins() & 4
                } else {
                    4
                };
                self.serial_control | si
            }
            SIODATA8 => u16::from(self.serial_send),
            RCNT => {
                // Normal-mode RCNT low pin samples are outside this subset.
                if self.rcnt & 0x8000 == 0 && address & 1 == 0 {
                    return None;
                }
                (self.rcnt & !15) | self.gpio_pins()
            }
            JOYCNT | JOY_RECV..=0x0400_0156 => 0, // Only reset-state Joybus accesses are supported.
            _ => return None,
        };
        Some(value.to_le_bytes()[(address & 1) as usize])
    }

    pub(super) fn write8(&mut self, address: u32, value: u8) {
        debug_assert!(Self::unsupported(address, value).is_none());
        if (WAVE_RAM..=WAVE_RAM + 15).contains(&address) {
            self.wave[(address - WAVE_RAM) as usize] = value;
            return;
        }
        if (SIODATA32..=SIODATA32 + 3).contains(&address) {
            self.serial_data[(address - SIODATA32) as usize] = value;
            return;
        }
        match address & !1 {
            SOUNDCNT_H => {
                self.sound_control = replace_byte(self.sound_control, address, value) & 0x770f
            }
            SOUNDBIAS => self.bias = replace_byte(self.bias, address, value) & 0xc3fe,
            SIOCNT => {
                self.serial_control = replace_byte(self.serial_control, address, value) & 0x500b
            }
            SIODATA8 if address & 1 == 0 => self.serial_send = value,
            RCNT => self.rcnt = replace_byte(self.rcnt, address, value) & 0x80ff,
            _ => {} // Disabled PSG writes and unused/read-only bits have no effect.
        }
    }
}
