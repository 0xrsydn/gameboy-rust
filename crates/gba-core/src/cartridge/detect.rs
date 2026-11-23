//! Save-type selection from the save library's ID string in the ROM.
//!
//! GBATEK ("GBA Cart Backup IDs"): Nintendo's save libraries carry a word-aligned ASCII ID,
//! `EEPROM_Vnnn`, `SRAM_Vnnn`, `SRAM_F_Vnnn`, `FLASH_Vnnn`, `FLASH512_Vnnn`, or `FLASH1M_Vnnn`,
//! where `nnn` is the library version. Emulators commonly select the save chip from it.
//! EEPROM IDs do not give the part size, so they select the size-from-first-request device.
//! The version digits are not required: public save tests (jsmolka's and Alyosha's) carry the
//! ID with spaces instead, and Mesen 2 selects from the ID alone. Alignment is still required.

use super::SaveDevice;

const IDS: [(&[u8], SaveDevice); 6] = [
    (b"EEPROM_V", SaveDevice::Eeprom),
    (b"SRAM_V", SaveDevice::Sram),
    (b"SRAM_F_V", SaveDevice::Sram),
    (b"FLASH_V", SaveDevice::Flash64),
    (b"FLASH512_V", SaveDevice::Flash64),
    (b"FLASH1M_V", SaveDevice::Flash128),
];

impl SaveDevice {
    /// The device named by the first word-aligned save-library ID, or `None` when the ROM
    /// carries none.
    pub fn detect(rom: &[u8]) -> SaveDevice {
        (0..rom.len())
            .step_by(4)
            .find_map(|offset| {
                let rest = &rom[offset..];
                IDS.iter()
                    .find_map(|&(id, device)| rest.starts_with(id).then_some(device))
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rom_with(id: &[u8], offset: usize) -> Vec<u8> {
        let mut rom = vec![0; 256];
        rom[offset..offset + id.len()].copy_from_slice(id);
        rom
    }

    #[test]
    fn each_library_id_selects_its_device() {
        for (id, device) in [
            (&b"EEPROM_V124"[..], SaveDevice::Eeprom),
            (b"SRAM_V113", SaveDevice::Sram),
            (b"SRAM_F_V102", SaveDevice::Sram),
            (b"FLASH_V126", SaveDevice::Flash64),
            (b"FLASH512_V131", SaveDevice::Flash64),
            (b"FLASH1M_V103", SaveDevice::Flash128),
        ] {
            assert_eq!(SaveDevice::detect(&rom_with(id, 0xc0)), device, "{id:?}");
        }
    }

    #[test]
    fn unaligned_ids_and_plain_roms_select_nothing() {
        assert_eq!(
            SaveDevice::detect(&rom_with(b"SRAM_V113", 0xc1)),
            SaveDevice::None
        );
        assert_eq!(SaveDevice::detect(&[]), SaveDevice::None);
        assert_eq!(SaveDevice::detect(&[0; 64]), SaveDevice::None);
    }

    #[test]
    fn ids_without_version_digits_still_select_as_in_the_public_save_tests() {
        for (id, device) in [
            (&b"SRAM_V  "[..], SaveDevice::Sram),
            (b"FLASH_V FLASH512_V  ", SaveDevice::Flash64),
            (b"FLASH1M_V   ", SaveDevice::Flash128),
            (b"SRAM_V11", SaveDevice::Sram),
        ] {
            assert_eq!(SaveDevice::detect(&rom_with(id, 0x40)), device, "{id:?}");
        }
    }

    #[test]
    fn the_first_id_in_rom_order_wins() {
        let mut rom = rom_with(b"FLASH1M_V103", 0x40);
        rom[0x80..0x89].copy_from_slice(b"SRAM_V113");
        assert_eq!(SaveDevice::detect(&rom), SaveDevice::Flash128);
    }
}
