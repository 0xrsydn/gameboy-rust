//! Optional Game Pak hardware. No host clock, filesystem, or ROM-byte mutation.

use crate::memory::MemoryError;

mod calendar;
mod flash;
mod rtc;
pub use calendar::{RtcDateTime, RtcError};
use flash::Flash;
pub(crate) use flash::SaveMutation;
pub use flash::{
    SaveDevice, SaveError, FLASH_ERASE_CYCLES, FLASH_PROGRAM_CYCLES, SAVE_END, SAVE_START,
};
use rtc::Rtc;

pub const GPIO_DATA: u32 = 0x0800_00c4;
pub const GPIO_DIRECTION: u32 = 0x0800_00c6;
pub const GPIO_CONTROL: u32 = 0x0800_00c8;

/// Explicit peripheral selection; ROM contents do not select a device.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CartridgeHardware {
    #[default]
    None,
    /// GPIO and RTC calendar/control. Time advances only through explicit caller updates.
    Rtc,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cartridge {
    gpio: Option<Gpio>,
    flash: Option<Flash>,
}

impl Cartridge {
    pub(crate) fn set_hardware(&mut self, hardware: CartridgeHardware) {
        self.gpio = (hardware == CartridgeHardware::Rtc).then(Gpio::default);
    }

    pub(crate) fn set_save_device(&mut self, device: SaveDevice) {
        self.flash = Flash::new(device);
    }

    pub(crate) fn save_device(&self) -> SaveDevice {
        self.flash.map_or(SaveDevice::None, Flash::device)
    }

    pub(crate) fn save_mapped(&self, address: u32) -> bool {
        self.flash.is_some() && (SAVE_START..=SAVE_END).contains(&address)
    }

    pub(crate) fn advance(&mut self, cycles: u32) {
        if let Some(flash) = self.flash.as_mut() {
            flash.advance(cycles);
        }
    }

    pub(crate) fn save_unfinished(&self) -> bool {
        self.flash.is_some_and(Flash::unfinished)
    }

    pub(crate) fn take_save_mutation(&mut self) -> Option<SaveMutation> {
        self.flash.as_mut().and_then(Flash::take_mutation)
    }

    pub(crate) fn read_save8(&self, address: u32, image: &[u8]) -> Result<u8, MemoryError> {
        self.flash.expect("mapped save device").read(address, image)
    }

    pub(crate) fn rtc_datetime(&self) -> Option<RtcDateTime> {
        self.gpio.map(|gpio| gpio.rtc.calendar)
    }

    pub(crate) fn set_rtc_datetime(&mut self, date: RtcDateTime) -> Result<(), RtcError> {
        self.gpio
            .as_mut()
            .ok_or(RtcError::NotAttached)?
            .rtc
            .calendar = date;
        Ok(())
    }

    pub(crate) fn advance_rtc_seconds(&mut self, seconds: u64) -> Result<(), RtcError> {
        self.gpio
            .as_mut()
            .ok_or(RtcError::NotAttached)?
            .rtc
            .calendar
            .advance(seconds);
        Ok(())
    }

    pub(crate) fn mapped(&self, address: u32) -> bool {
        self.save_mapped(address)
            || (self.gpio.is_some() && (GPIO_DATA..GPIO_CONTROL + 2).contains(&address))
    }

    /// Disabled reads expose the supplied ROM bytes, not a zero-filled replacement.
    pub(crate) fn read8(&self, address: u32) -> Option<u8> {
        let gpio = self.gpio?;
        if !(GPIO_DATA..GPIO_CONTROL + 2).contains(&address) || !gpio.readable {
            return None;
        }
        let value = match address & !1 {
            GPIO_DATA => gpio.pins(),
            GPIO_DIRECTION => gpio.direction,
            GPIO_CONTROL => 1,
            _ => unreachable!(),
        };
        Some(if address & 1 == 0 { value } else { 0 })
    }

    /// Apply one access to a copy first. A rejected second halfword changes nothing.
    pub(crate) fn write(
        &mut self,
        address: u32,
        bytes: &[u8],
        image: &[u8],
    ) -> Result<(), MemoryError> {
        if !self.mapped(address) {
            return Ok(()); // Non-cartridge accesses are validated by the bus.
        }
        if self.save_mapped(address) {
            if bytes.len() != 1 {
                return Err(MemoryError::UnsupportedCartridgeAccess {
                    address,
                    operation: "Flash non-byte write",
                });
            }
            return self.flash.as_mut().unwrap().write(address, bytes[0], image);
        }
        let fail = |at, value, operation| MemoryError::UnsupportedIo {
            address: at,
            value,
            operation,
        };
        if bytes.len() == 1 {
            return Err(fail(address, bytes[0], "Game Pak GPIO byte write"));
        }
        let mut next = self.gpio.unwrap();
        for (i, pair) in bytes.chunks_exact(2).enumerate() {
            let at = address + i as u32 * 2;
            let value = pair[0]; // Registers use at most the low four bits.
            match at {
                GPIO_DATA | GPIO_DIRECTION => {
                    if at == GPIO_DATA {
                        next.latch = value & 15;
                    } else {
                        next.direction = value & 15;
                    }
                    next.rtc
                        .update(next.latch & next.direction, next.direction)
                        .map_err(|operation| fail(at, value, operation))?;
                }
                GPIO_CONTROL => next.readable = value & 1 != 0,
                _ => return Err(MemoryError::ReadOnly(at)),
            }
        }
        self.gpio = Some(next);
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Gpio {
    latch: u8,
    direction: u8,
    readable: bool,
    rtc: Rtc,
}

impl Gpio {
    fn pins(self) -> u8 {
        // RTC input defaults: SIO high outside a read; SCK/CS/unused low.
        (self.latch & self.direction) | (u8::from(self.rtc.output) << 1 & !self.direction)
    }
}
