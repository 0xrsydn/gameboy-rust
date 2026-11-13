//! Optional Game Pak hardware. No host clock, filesystem, or ROM-byte mutation.

use crate::memory::MemoryError;
use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};

mod calendar;
mod eeprom;
mod flash;
mod rtc;
mod sram;
pub use calendar::{RtcDateTime, RtcError};
use eeprom::Eeprom;
pub use eeprom::EEPROM_WRITE_CYCLES;
use flash::Flash;
pub(crate) use flash::SaveMutation;
pub use flash::{
    SaveDevice, SaveError, FLASH_ERASE_CYCLES, FLASH_PROGRAM_CYCLES, SAVE_END, SAVE_START,
};
use rtc::Rtc;
use sram::Sram;

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
    sram: Option<Sram>,
    eeprom: Option<Eeprom>,
}

impl Cartridge {
    pub(crate) fn set_hardware(&mut self, hardware: CartridgeHardware) {
        self.gpio = (hardware == CartridgeHardware::Rtc).then(Gpio::default);
    }

    /// The ROM length selects the EEPROM window: carts above 16 MiB decode only its top 256 bytes.
    pub(crate) fn set_save_device(&mut self, device: SaveDevice, rom_len: usize) {
        self.flash = Flash::new(device);
        self.sram = (device == SaveDevice::Sram).then(Sram::default);
        self.eeprom = Eeprom::new(device, rom_len);
    }

    pub(crate) fn save_device(&self) -> SaveDevice {
        if self.sram.is_some() {
            return SaveDevice::Sram;
        }
        if let Some(eeprom) = self.eeprom {
            return eeprom.device();
        }
        self.flash.map_or(SaveDevice::None, Flash::device)
    }

    pub(crate) fn eeprom_mapped(&self, address: u32) -> bool {
        self.eeprom.is_some_and(|eeprom| eeprom.mapped(address))
    }

    /// One EEPROM halfword read. CPU reads poll status; other reads shift out data.
    pub(crate) fn read_eeprom16(&mut self, address: u32, cpu: bool) -> Result<u16, MemoryError> {
        self.eeprom
            .as_mut()
            .expect("mapped EEPROM")
            .read(address, cpu)
    }

    pub(crate) fn save_mapped(&self, address: u32) -> bool {
        (self.flash.is_some() || self.sram.is_some()) && (SAVE_START..=SAVE_END).contains(&address)
    }

    /// SRAM sits on an 8-bit bus that sees the unaligned address of wider accesses.
    pub(crate) fn sram_mapped(&self, address: u32) -> bool {
        self.sram.is_some() && self.save_mapped(address)
    }

    /// Flash reads are restricted to work-RAM code; SRAM has no such limit.
    pub(crate) fn flash_mapped(&self, address: u32) -> bool {
        self.flash.is_some() && self.save_mapped(address)
    }

    pub(crate) fn advance(&mut self, cycles: u32) {
        if let Some(flash) = self.flash.as_mut() {
            flash.advance(cycles);
        }
        if let Some(eeprom) = self.eeprom.as_mut() {
            eeprom.advance(cycles);
        }
    }

    pub(crate) fn save_unfinished(&self) -> bool {
        self.flash.is_some_and(Flash::unfinished)
            || self.sram.is_some_and(Sram::unfinished)
            || self.eeprom.is_some_and(Eeprom::unfinished)
    }

    /// A completed EEPROM write: its first byte offset and the eight block bytes.
    pub(crate) fn take_save_block(&mut self) -> Option<(usize, [u8; 8])> {
        self.eeprom.as_mut().and_then(Eeprom::take_block)
    }

    pub(crate) fn take_save_mutation(&mut self) -> Option<SaveMutation> {
        if let Some(sram) = self.sram.as_mut() {
            return sram.take_mutation();
        }
        self.flash.as_mut().and_then(Flash::take_mutation)
    }

    pub(crate) fn read_save8(&self, address: u32, image: &[u8]) -> Result<u8, MemoryError> {
        if let Some(sram) = self.sram {
            return Ok(sram.read(address, image));
        }
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
            || self.eeprom_mapped(address)
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
    /// `cpu` marks CPU stores, which cannot carry EEPROM bitstreams.
    pub(crate) fn write(
        &mut self,
        address: u32,
        bytes: &[u8],
        image: &[u8],
        cpu: bool,
    ) -> Result<(), MemoryError> {
        if !self.mapped(address) {
            return Ok(()); // Non-cartridge accesses are validated by the bus.
        }
        if self.eeprom_mapped(address) {
            if bytes.len() != 2 {
                return Err(MemoryError::UnsupportedCartridgeAccess {
                    address,
                    operation: "EEPROM non-halfword write",
                });
            }
            let value = u16::from_le_bytes([bytes[0], bytes[1]]);
            return self
                .eeprom
                .as_mut()
                .unwrap()
                .write(address, value, cpu, image);
        }
        if self.save_mapped(address) {
            if bytes.len() != 1 {
                return Err(MemoryError::UnsupportedCartridgeAccess {
                    address,
                    operation: if self.sram.is_some() {
                        "SRAM non-byte write"
                    } else {
                        "Flash non-byte write"
                    },
                });
            }
            if let Some(sram) = self.sram.as_mut() {
                return sram.write(address, bytes[0]);
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

impl Cartridge {
    /// Host configuration, not state: cartridge hardware then save device, one byte each.
    pub(crate) fn setup_identity(&self) -> [u8; 2] {
        let device = self.save_device();
        [
            u8::from(self.gpio.is_some()),
            match device {
                SaveDevice::None => 0,
                SaveDevice::Flash64 => 1,
                SaveDevice::Flash128 => 2,
                SaveDevice::Sram => 3,
                SaveDevice::Eeprom512 => 4,
                SaveDevice::Eeprom8k => 5,
            },
        ]
    }
}

impl Snapshot for Cartridge {
    fn save(&self, writer: &mut Writer) {
        let Self {
            gpio,
            flash,
            sram,
            eeprom,
        } = self;
        writer.tag(b"CART");
        writer.raw(&self.setup_identity());
        if let Some(gpio) = gpio {
            gpio.save(writer);
        }
        if let Some(flash) = flash {
            flash.save(writer);
        }
        if let Some(sram) = sram {
            sram.save(writer);
        }
        if let Some(eeprom) = eeprom {
            eeprom.save(writer);
        }
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let target_device = self.save_device();
        let Self {
            gpio,
            flash,
            sram,
            eeprom,
        } = self;
        reader.tag(b"CART")?;

        let hardware = reader.u8()?;
        if hardware > 1 {
            return Err(SnapshotError::Corrupt("cartridge hardware"));
        }
        if hardware != u8::from(gpio.is_some()) {
            return Err(SnapshotError::ConfigMismatch("cartridge hardware"));
        }

        let save_device = match reader.u8()? {
            0 => SaveDevice::None,
            1 => SaveDevice::Flash64,
            2 => SaveDevice::Flash128,
            3 => SaveDevice::Sram,
            4 => SaveDevice::Eeprom512,
            5 => SaveDevice::Eeprom8k,
            _ => return Err(SnapshotError::Corrupt("save device")),
        };
        if save_device != target_device {
            return Err(SnapshotError::ConfigMismatch("save device"));
        }

        if let Some(gpio) = gpio {
            gpio.load(reader)?;
        }
        if let Some(flash) = flash {
            flash.load(reader)?;
        }
        if let Some(sram) = sram {
            sram.load(reader)?;
        }
        if let Some(eeprom) = eeprom {
            eeprom.load(reader)?;
        }
        Ok(())
    }
}

impl Snapshot for Gpio {
    fn save(&self, writer: &mut Writer) {
        let Self {
            latch,
            direction,
            readable,
            rtc,
        } = self;
        writer.tag(b"GPIO");
        writer.u8(*latch);
        writer.u8(*direction);
        writer.bool(*readable);
        rtc.save(writer);
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self {
            latch,
            direction,
            readable,
            rtc,
        } = self;
        reader.tag(b"GPIO")?;
        let saved_latch = reader.u8()?;
        let saved_direction = reader.u8()?;
        if saved_latch & !0x0f != 0 || saved_direction & !0x0f != 0 {
            return Err(SnapshotError::Corrupt("GPIO pins"));
        }
        let saved_readable = reader.bool()?;
        rtc.load(reader)?;
        *latch = saved_latch;
        *direction = saved_direction;
        *readable = saved_readable;
        Ok(())
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    fn encoded<T: Snapshot>(value: &T) -> Vec<u8> {
        let mut writer = Writer::new();
        value.save(&mut writer);
        writer.finish().unwrap()
    }

    fn configured_cartridge(hardware: CartridgeHardware, device: SaveDevice) -> Cartridge {
        let mut cartridge = Cartridge::default();
        cartridge.set_hardware(hardware);
        cartridge.set_save_device(device, 0);
        cartridge
    }

    #[test]
    fn cartridge_snapshot_round_trips_and_checks_configuration() {
        let mut original = configured_cartridge(CartridgeHardware::Rtc, SaveDevice::Flash128);
        let gpio = original.gpio.as_mut().unwrap();
        gpio.latch = 0x0b;
        gpio.direction = 0x05;
        gpio.readable = true;
        let bytes = encoded(&original);

        let mut restored = configured_cartridge(CartridgeHardware::Rtc, SaveDevice::Flash128);
        restored.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(restored, original);

        for end in 0..bytes.len() {
            let mut target = configured_cartridge(CartridgeHardware::Rtc, SaveDevice::Flash128);
            assert!(target.load(&mut Reader::new(&bytes[..end])).is_err());
        }

        let mut no_rtc = configured_cartridge(CartridgeHardware::None, SaveDevice::Flash128);
        assert_eq!(
            no_rtc.load(&mut Reader::new(&bytes)),
            Err(SnapshotError::ConfigMismatch("cartridge hardware"))
        );
        let mut other_save = configured_cartridge(CartridgeHardware::Rtc, SaveDevice::Flash64);
        assert_eq!(
            other_save.load(&mut Reader::new(&bytes)),
            Err(SnapshotError::ConfigMismatch("save device"))
        );

        let mut invalid_identity = bytes.clone();
        invalid_identity[4] = 0xff;
        assert_eq!(
            configured_cartridge(CartridgeHardware::Rtc, SaveDevice::Flash128)
                .load(&mut Reader::new(&invalid_identity)),
            Err(SnapshotError::Corrupt("cartridge hardware"))
        );

        let empty = Cartridge::default();
        let empty_bytes = encoded(&empty);
        let mut empty_restored = Cartridge::default();
        empty_restored.load(&mut Reader::new(&empty_bytes)).unwrap();
        assert_eq!(empty_restored, empty);
    }

    #[test]
    fn gpio_snapshot_round_trips_every_gpio_latch() {
        let mut gpio = Gpio {
            latch: 0x0d,
            direction: 0x06,
            readable: true,
            ..Gpio::default()
        };
        gpio.rtc.update(5, 2).unwrap();
        gpio.rtc.update(6, 2).unwrap();
        gpio.rtc.update(7, 2).unwrap();
        let bytes = encoded(&gpio);

        let mut restored = Gpio::default();
        restored.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(restored, gpio);

        for end in 0..bytes.len() {
            assert!(Gpio::default()
                .load(&mut Reader::new(&bytes[..end]))
                .is_err());
        }

        let mut corrupt = bytes;
        corrupt[4] = 0x10;
        assert_eq!(
            Gpio::default().load(&mut Reader::new(&corrupt)),
            Err(SnapshotError::Corrupt("GPIO pins"))
        );
    }
}
