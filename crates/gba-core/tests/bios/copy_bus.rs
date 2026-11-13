//! Copy services execute real bus accesses, including ignored BIOS destinations.
use super::*;
use gba_core::cartridge::{SaveDevice, SAVE_START};

#[test]
fn copy_and_fill_to_bios_return_without_modifying_firmware() {
    for thumb in [false, true] {
        for (service, width) in [(0x0b, 0), (0x0b, 1 << 26), (0x0c, 0)] {
            for fill in [0, 1 << 24] {
                let (mut machine, return_pc) = call(service, thumb, [SOURCE, 0, 28 | width | fill]);
                for index in 0..32 {
                    machine
                        .memory_mut()
                        .write32(SOURCE + index * 4, 0x1234_abcd ^ index)
                        .unwrap();
                }
                let before = machine.cpu().clone();
                let cycles = machine.cycles();
                finish(&mut machine, return_pc);
                assert_eq!(&machine.cpu().registers()[..15], &before.registers()[..15]);
                assert_eq!(machine.cpu().cpsr(), before.cpsr());
                assert!(machine.cycles() > cycles);
                for (address, expected) in bios::image().into_iter().enumerate() {
                    assert_eq!(machine.memory().read8(address as u32).unwrap(), expected);
                }
                assert_eq!(
                    machine.memory_mut().write16(0, 0),
                    Err(MemoryError::ReadOnly(0))
                );
            }
        }
    }
}

#[test]
fn ignored_bios_destination_does_not_bypass_invalid_source_reads() {
    for service in [0x0b, 0x0c] {
        let (mut machine, _) = call(service, true, [0x0e00_0000, 0, 1]);
        let error = (0..200)
            .find_map(|_| machine.step().err())
            .expect("source read must still fail");
        assert_eq!(
            error,
            MachineError::Cpu(CpuError::Memory(MemoryError::Unmapped(0x0e00_0000)))
        );
        let cpu = machine.cpu().clone();
        let cycles = machine.cycles();
        assert_eq!(machine.step(), Err(error));
        assert_eq!(machine.cpu(), &cpu);
        assert_eq!(machine.cycles(), cycles);
    }
}

fn with_sram(machine: &mut Machine, image: &[u8]) {
    let memory = machine.memory_mut();
    memory.set_save_device(SaveDevice::Sram);
    let mut full = memory.save_image().unwrap().to_vec();
    full[..image.len()].copy_from_slice(image);
    memory.load_save_image(&full).unwrap();
}

// Expected words are the mGBA test suite's hardware results ("SRAM load", "swi B"/"swi C"):
// the firmware passes misaligned pointers to the CPU, and SRAM's 8-bit bus sees the low bits.
#[test]
fn misaligned_copies_from_sram_read_the_addressed_byte_lanes() {
    for (service, width, offset, expected) in [
        (0x0b, 0, 1, 0x0065_0061), // Each odd LDRH rotates its repeated byte.
        (0x0b, 1 << 26, 1, 0x6161_6161),
        (0x0b, 1 << 26, 3, 0x6565_6565),
        (0x0c, 0, 1, 0x6161_6161),
        (0x0c, 0, 2, 0x6d6d_6d6d),
    ] {
        let (mut machine, return_pc) = call(service, false, [SAVE_START + offset, DEST, 8 | width]);
        with_sram(&mut machine, b"Game Boy");
        finish(&mut machine, return_pc);
        assert_eq!(
            machine.memory().read32(DEST).unwrap(),
            expected,
            "service {service:#x} width {width:#x} +{offset}"
        );
    }
}

#[test]
fn misaligned_word_copies_to_sram_store_the_addressed_byte_lanes() {
    for service in [0x0b, 0x0c] {
        let (mut machine, return_pc) =
            call(service, false, [SOURCE, SAVE_START + 1, 2 | (1 << 26)]);
        with_sram(&mut machine, &[]);
        machine.memory_mut().write32(SOURCE, 0xa5b6_c7d8).unwrap();
        machine
            .memory_mut()
            .write32(SOURCE + 4, 0x1122_3344)
            .unwrap();
        finish(&mut machine, return_pc);
        // Byte 0 stays erased, as the suite's "SRAM store" results require.
        assert_eq!(
            &machine.memory().save_image().unwrap()[..6],
            &[0xff, 0xc7, 0xff, 0xff, 0xff, 0x33],
            "service {service:#x}"
        );
    }
}
