//! Disconnected serial state, including external-clock requests that cannot progress.

use super::replace_byte;

pub const SIODATA32: u32 = 0x0400_0120;
pub const SIOCNT: u32 = 0x0400_0128;
pub const SIODATA8: u32 = 0x0400_012a;
pub const RCNT: u32 = 0x0400_0134;
pub const JOYCNT: u32 = 0x0400_0140;
pub const JOY_RECV: u32 = 0x0400_0150;
pub const JOY_TRANS: u32 = 0x0400_0154;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Inactive {
    serial_data: [u8; 4],
    serial_send: u8,
    serial_control: u16,
    rcnt: u16,
}

impl Inactive {
    pub(super) fn mapped(address: u32) -> bool {
        matches!(address,
            SIODATA32..=0x0400_0123 | SIOCNT..=0x0400_012b |
            RCNT..=0x0400_0135 | JOYCNT..=0x0400_0141 |
            JOY_RECV..=0x0400_0157)
    }

    /// Value-dependent rejection is independent of device state, so batch
    /// stores can validate all values before committing any RAM/I/O writes.
    /// RCNT mode and IRQ-enable bits share the high byte. Each high-byte write
    /// supplies the complete selection, independent of the previous low byte.
    pub(super) fn unsupported(address: u32, value: u8) -> Option<&'static str> {
        match address {
            // Clock source and start share the low byte. With no external
            // clock edges, an external-clock request only latches start/busy.
            SIOCNT if value & 0x81 == 0x81 => Some("internally clocked serial transfer"),
            0x0400_0129 if value & 0x20 != 0 => Some("multiplayer/UART serial mode"),
            0x0400_0135 if value & 0xc0 == 0xc0 => Some("Joybus serial mode"),
            0x0400_0135 if value & 0x81 == 0x81 => Some("GPIO serial interrupt enable"),
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
        if (SIODATA32..=SIODATA32 + 3).contains(&address) {
            return Some(self.serial_data[(address - SIODATA32) as usize]);
        }
        let value = match address & !1 {
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
        if (SIODATA32..=SIODATA32 + 3).contains(&address) {
            self.serial_data[(address - SIODATA32) as usize] = value;
            return;
        }
        match address & !1 {
            SIOCNT => {
                self.serial_control = replace_byte(self.serial_control, address, value) & 0x508b
            }
            SIODATA8 if address & 1 == 0 => self.serial_send = value,
            RCNT => self.rcnt = replace_byte(self.rcnt, address, value) & 0xc1ff,
            _ => {} // Unused/read-only bits have no effect.
        }
    }
}
