//! Serial EEPROM on Game Pak data bit 0, following GBATEK's bitstream description.
//! Requests arrive one bit per halfword: `11` + address + `0` reads a 64-bit block, and
//! `10` + address + 64 data bits + `0` writes one. A read returns 4 ignored bits, then the
//! block MSB first. A write completes after a nominal delay; until then status reads 0.
//! Block bytes are stored in transfer order, the first data bit as byte 0 bit 7.

use super::SaveDevice;
use crate::memory::MemoryError;
use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};

/// GBATEK: "ca. 108368 clock cycles (ca. 6.5ms)" to erase and program one block.
pub const EEPROM_WRITE_CYCLES: u32 = 108_368;
const READ_BITS: u8 = 68;
/// Carts with ROM above 16 MiB decode EEPROM only in the top 256 bytes of the 0x0d window.
const LARGE_ROM: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Eeprom {
    large: bool,
    start: u32,
    input: u128,
    count: u8,
    output: Option<Readout>,
    busy: Option<Busy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Readout {
    data: u64,
    remaining: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Busy {
    block: u16,
    data: u64,
    remaining: u32,
}

impl Eeprom {
    pub(super) fn new(device: SaveDevice, rom_len: usize) -> Option<Self> {
        let large = match device {
            SaveDevice::Eeprom512 => false,
            SaveDevice::Eeprom8k => true,
            _ => return None,
        };
        Some(Self {
            large,
            start: if rom_len > LARGE_ROM {
                0x0dff_ff00
            } else {
                0x0d00_0000
            },
            input: 0,
            count: 0,
            output: None,
            busy: None,
        })
    }

    pub(super) fn device(self) -> SaveDevice {
        if self.large {
            SaveDevice::Eeprom8k
        } else {
            SaveDevice::Eeprom512
        }
    }

    pub(super) fn mapped(self, address: u32) -> bool {
        (self.start..=0x0dff_ffff).contains(&address)
    }

    fn address_bits(self) -> u8 {
        if self.large {
            14
        } else {
            6
        }
    }

    /// Block index from a request's address field. 8 KiB parts decode the low 10 bits.
    fn block(self, address: u128) -> u16 {
        (address as u16) & if self.large { 0x3ff } else { 0x3f }
    }

    /// One halfword read. CPU reads only poll status; DMA reads also shift out data.
    pub(super) fn read(&mut self, address: u32, cpu: bool) -> Result<u16, MemoryError> {
        if let Some(output) = self.output.as_mut() {
            if cpu {
                return Err(MemoryError::UnsupportedCartridgeAccess {
                    address,
                    operation: "EEPROM CPU read during a data read-out",
                });
            }
            output.remaining -= 1;
            let bit = if output.remaining < 64 {
                (output.data >> output.remaining) & 1
            } else {
                0 // Four leading bits carry no data.
            };
            if output.remaining == 0 {
                self.output = None;
            }
            return Ok(bit as u16);
        }
        // Bits 1-15 read as zero here; hardware drives only bit 0.
        Ok(u16::from(self.busy.is_none()))
    }

    /// One halfword of a request bitstream. Only bit 0 is used.
    pub(super) fn write(
        &mut self,
        address: u32,
        value: u16,
        cpu: bool,
        image: &[u8],
    ) -> Result<(), MemoryError> {
        let fail = |operation| MemoryError::UnsupportedIo {
            address,
            value: value as u8,
            operation,
        };
        if cpu {
            // GBATEK: bitstreams need DMA3; manual STRH transfers do not work.
            return Err(fail("EEPROM CPU bitstream write"));
        }
        if self.busy.is_some() {
            return Err(fail("EEPROM write while busy"));
        }
        if self.output.is_some() {
            return Err(fail("EEPROM request during a data read-out"));
        }
        let mut next = *self;
        next.input = (next.input << 1) | u128::from(value & 1);
        next.count += 1;
        if next.count == 1 && value & 1 == 0 {
            return Err(fail("EEPROM request without a leading 1 bit"));
        }
        let address_bits = self.address_bits();
        let read_bits = 2 + address_bits + 1;
        let write_bits = 2 + address_bits + 64 + 1;
        let reading = next.count >= 2 && (next.input >> (next.count - 2)) & 1 == 1;
        let complete = if reading { read_bits } else { write_bits };
        if next.count == complete {
            if next.input & 1 != 0 {
                return Err(fail("EEPROM request without a trailing 0 bit"));
            }
            if reading {
                let block = next.block(next.input >> 1);
                let offset = usize::from(block) * 8;
                let mut bytes = [0; 8];
                bytes.copy_from_slice(&image[offset..offset + 8]);
                next.output = Some(Readout {
                    data: u64::from_be_bytes(bytes),
                    remaining: READ_BITS,
                });
            } else {
                next.busy = Some(Busy {
                    block: next.block(next.input >> 65),
                    data: (next.input >> 1) as u64,
                    remaining: EEPROM_WRITE_CYCLES,
                });
            }
            next.input = 0;
            next.count = 0;
        }
        *self = next;
        Ok(())
    }

    pub(super) fn advance(&mut self, cycles: u32) {
        if let Some(busy) = self.busy.as_mut() {
            busy.remaining = busy.remaining.saturating_sub(cycles);
        }
    }

    pub(super) fn unfinished(self) -> bool {
        self.count != 0 || self.output.is_some() || self.busy.is_some()
    }

    /// A completed write: its first byte offset and the eight block bytes.
    pub(super) fn take_block(&mut self) -> Option<(usize, [u8; 8])> {
        let busy = self.busy.filter(|busy| busy.remaining == 0)?;
        self.busy = None;
        Some((usize::from(busy.block) * 8, busy.data.to_be_bytes()))
    }
}

impl Snapshot for Eeprom {
    fn save(&self, writer: &mut Writer) {
        let Self {
            large,
            start,
            input,
            count,
            output,
            busy,
        } = self;
        writer.tag(b"EEPR");
        writer.bool(*large);
        writer.u32(*start);
        writer.u64((*input >> 64) as u64);
        writer.u64(*input as u64);
        writer.u8(*count);
        writer.option(output.as_ref(), |output, writer| {
            writer.u64(output.data);
            writer.u8(output.remaining);
        });
        writer.option(busy.as_ref(), |busy, writer| {
            writer.u32(u32::from(busy.block));
            writer.u64(busy.data);
            writer.u32(busy.remaining);
        });
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        reader.tag(b"EEPR")?;
        let large = reader.bool()?;
        let start = reader.u32()?;
        let input = (u128::from(reader.u64()?) << 64) | u128::from(reader.u64()?);
        let count = reader.u8()?;
        let output = reader.option(|reader| {
            Ok(Readout {
                data: reader.u64()?,
                remaining: reader.u8()?,
            })
        })?;
        let busy = reader.option(|reader| {
            let block =
                u16::try_from(reader.u32()?).map_err(|_| SnapshotError::Corrupt("EEPROM block"))?;
            Ok(Busy {
                block,
                data: reader.u64()?,
                remaining: reader.u32()?,
            })
        })?;
        if large != self.large || start != self.start {
            return Err(SnapshotError::ConfigMismatch("save device"));
        }
        let restored = Self {
            large,
            start,
            input,
            count,
            output,
            busy,
        };
        let write_bits = 2 + restored.address_bits() + 64 + 1;
        if count >= write_bits
            || input >> count != 0
            || output.is_some_and(|o| o.remaining == 0 || o.remaining > READ_BITS)
            || busy.is_some_and(|b| b.block != restored.block(u128::from(b.block)))
            || (output.is_some() as u8 + busy.is_some() as u8 + (count != 0) as u8) > 1
        {
            return Err(SnapshotError::Corrupt("EEPROM state"));
        }
        *self = restored;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(eeprom: &mut Eeprom, value: u128, count: u8, image: &[u8]) {
        for shift in (0..count).rev() {
            let bit = ((value >> shift) & 1) as u16;
            eeprom.write(0x0d00_0000, bit, false, image).unwrap();
        }
    }

    #[test]
    fn small_and_large_parts_read_four_dummy_bits_then_the_block_msb_first() {
        for (device, address_bits, block) in [
            (SaveDevice::Eeprom512, 6, 0x3f_u128),
            (SaveDevice::Eeprom8k, 14, 0x3ff),
        ] {
            let mut image = vec![0xff; device.capacity()];
            let offset = block as usize * 8;
            image[offset..offset + 8].copy_from_slice(&0x0123_4567_89ab_cdef_u64.to_be_bytes());
            let mut eeprom = Eeprom::new(device, 0).unwrap();
            bits(
                &mut eeprom,
                (0b11 << (address_bits + 1)) | (block << 1),
                address_bits + 3,
                &image,
            );
            let mut value = 0_u64;
            for index in 0..68 {
                let bit = eeprom.read(0x0d00_0000, false).unwrap();
                if index < 4 {
                    assert_eq!(bit, 0);
                } else {
                    value = (value << 1) | u64::from(bit);
                }
            }
            assert_eq!(value, 0x0123_4567_89ab_cdef);
            assert!(!eeprom.unfinished());
            assert_eq!(eeprom.read(0x0d00_0000, true).unwrap(), 1); // Idle and ready.
        }
    }

    #[test]
    fn writes_report_busy_until_the_nominal_delay_then_yield_the_block() {
        let image = vec![0xff; SaveDevice::Eeprom512.capacity()];
        let mut eeprom = Eeprom::new(SaveDevice::Eeprom512, 0).unwrap();
        let data = 0xfedc_ba98_7654_3210_u128;
        bits(
            &mut eeprom,
            (0b10 << 71) | (5 << 65) | (data << 1),
            73,
            &image,
        );
        assert_eq!(eeprom.read(0x0d00_0000, true).unwrap(), 0);
        eeprom.advance(EEPROM_WRITE_CYCLES - 1);
        assert_eq!(eeprom.take_block(), None);
        eeprom.advance(1);
        assert_eq!(
            eeprom.take_block(),
            Some((40, 0xfedc_ba98_7654_3210_u64.to_be_bytes()))
        );
        assert_eq!(eeprom.read(0x0d00_0000, true).unwrap(), 1);
    }

    #[test]
    fn malformed_requests_cpu_stores_and_busy_writes_are_diagnostics() {
        let image = vec![0xff; SaveDevice::Eeprom512.capacity()];
        let mut eeprom = Eeprom::new(SaveDevice::Eeprom512, 0).unwrap();
        assert!(eeprom.write(0x0d00_0000, 0, false, &image).is_err());
        assert!(eeprom.write(0x0d00_0000, 1, true, &image).is_err());
        let mut trailing = Eeprom::new(SaveDevice::Eeprom512, 0).unwrap();
        bits(&mut trailing, 0b11_000000, 8, &image);
        assert!(trailing.write(0x0d00_0000, 1, false, &image).is_err());
        bits(&mut eeprom, 0b10 << 71, 73, &image);
        assert!(eeprom.write(0x0d00_0000, 1, false, &image).is_err());
    }

    #[test]
    fn roms_above_16_mib_map_only_the_top_256_bytes() {
        let small = Eeprom::new(SaveDevice::Eeprom8k, LARGE_ROM).unwrap();
        assert!(small.mapped(0x0d00_0000));
        let large = Eeprom::new(SaveDevice::Eeprom8k, LARGE_ROM + 1).unwrap();
        assert!(!large.mapped(0x0dff_feff));
        assert!(large.mapped(0x0dff_ff00));
    }

    #[test]
    fn snapshots_round_trip_mid_request() {
        let image = vec![0xff; SaveDevice::Eeprom8k.capacity()];
        let mut eeprom = Eeprom::new(SaveDevice::Eeprom8k, 0).unwrap();
        bits(&mut eeprom, 0b10_0000, 6, &image);
        let mut writer = Writer::new();
        eeprom.save(&mut writer);
        let bytes = writer.finish().unwrap();
        let mut restored = Eeprom::new(SaveDevice::Eeprom8k, 0).unwrap();
        restored.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(restored, eeprom);
        let mut other = Eeprom::new(SaveDevice::Eeprom512, 0).unwrap();
        assert_eq!(
            other.load(&mut Reader::new(&bytes)),
            Err(SnapshotError::ConfigMismatch("save device"))
        );
    }
}
