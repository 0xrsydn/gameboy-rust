//! Original SRAM accesses and supplied save bytes; no game assets or save files.
use gba_core::{
    cartridge::{SaveDevice, SaveError, SAVE_END, SAVE_START},
    cpu::Cpu,
    dma::DMA_BASE,
    machine::{Machine, MachineError},
    memory::{Memory, MemoryError, ROM_START},
    snapshot::SnapshotError,
};

const SIZE: usize = 32 * 1024;

fn words(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn memory(rom: &[u32]) -> Memory {
    let mut bus = Memory::new(words(rom)).unwrap();
    bus.set_save_device(SaveDevice::Sram);
    bus
}

/// LDR r0,=address; then `instruction`; then loop.
fn program(address: u32, instruction: u32) -> Machine {
    let mut bus = memory(&[0xe59f_0004, instruction, 0xeaff_fffe, address]);
    let mut cpu = Cpu::new(ROM_START);
    cpu.step(&mut bus).unwrap();
    Machine::new(cpu, bus)
}

#[test]
fn selection_starts_erased_and_loads_only_exact_images() {
    let mut bus = memory(&[0xeaff_fffe]);
    assert_eq!(SaveDevice::Sram.capacity(), SIZE);
    assert_eq!(bus.save_image().unwrap(), &[0xff; SIZE][..]);
    assert!(!bus.save_modified());
    assert_eq!(
        bus.load_save_image(&[0; SIZE - 1]),
        Err(SaveError::InvalidSize {
            expected: SIZE,
            actual: SIZE - 1
        })
    );
    let image: Vec<u8> = (0..SIZE).map(|i| (i * 7) as u8).collect();
    bus.load_save_image(&image).unwrap();
    assert_eq!(bus.read8(SAVE_START + 3).unwrap(), 21);
    assert!(!bus.save_modified());
}

#[test]
fn byte_writes_commit_immediately_and_mirror_every_32_kib() {
    let mut bus = memory(&[0xeaff_fffe]);
    bus.write8(SAVE_START + 0x8005, 0x42).unwrap();
    assert_eq!(bus.read8(SAVE_START + 5).unwrap(), 0x42);
    assert_eq!(bus.read8(SAVE_END).unwrap(), 0xff); // Mirror of offset 0x7fff.
    assert_eq!(bus.save_image().unwrap()[5], 0x42);
    assert!(bus.save_modified());
    assert!(!bus.save_write_pending());
    // Writing the stored value again does not mark an unchanged image as modified.
    let mut bus = memory(&[0xeaff_fffe]);
    bus.write8(SAVE_START, 0xff).unwrap();
    assert!(!bus.save_modified());
    assert_eq!(
        bus.read8(SAVE_END + 1),
        Err(MemoryError::Unmapped(SAVE_END + 1))
    );
}

#[test]
fn host_halfword_and_word_accesses_remain_diagnostics() {
    let mut bus = memory(&[0xeaff_fffe]);
    for result in [
        bus.read16(SAVE_START).map(|_| ()),
        bus.read32(SAVE_START).map(|_| ()),
    ] {
        assert_eq!(
            result,
            Err(MemoryError::UnsupportedCartridgeAccess {
                address: SAVE_START,
                operation: "SRAM non-byte read",
            })
        );
    }
    for result in [bus.write16(SAVE_START, 0), bus.write32(SAVE_START, 0)] {
        assert_eq!(
            result,
            Err(MemoryError::UnsupportedCartridgeAccess {
                address: SAVE_START,
                operation: "SRAM non-byte write",
            })
        );
    }
    assert!(!bus.save_modified());
}

#[test]
fn cpu_byte_stores_and_loads_work_from_rom_code() {
    let mut machine = program(SAVE_START + 9, 0xe5c0_0000); // STRB r0,[r0]
    machine.step().unwrap();
    assert_eq!(machine.memory().save_image().unwrap()[9], 0x09);
    assert!(machine.memory().save_modified());
    assert!(!machine.memory().save_write_pending());

    let mut machine = program(SAVE_START + 9, 0xe5d0_1000); // LDRB r1,[r0]
    machine.memory_mut().write8(SAVE_START + 9, 0x77).unwrap();
    machine.step().unwrap();
    assert_eq!(machine.cpu().registers()[1], 0x77);
}

#[test]
fn byte_swap_reads_the_old_value_then_stores_atomically() {
    let mut machine = program(SAVE_START, 0xe140_1090); // SWPB r1,r0,[r0]
    machine.memory_mut().write8(SAVE_START, 0x33).unwrap();
    machine.step().unwrap();
    assert_eq!(machine.cpu().registers()[1], 0x33);
    assert_eq!(machine.memory().read8(SAVE_START).unwrap(), 0x00);
}

/// LDR r0,=address; LDR r1,=value; then `instruction`; then loop.
fn program_with(address: u32, value: u32, instruction: u32, image: &[u8]) -> Machine {
    let mut bus = memory(&[
        0xe59f_0008,
        0xe59f_1008,
        instruction,
        0xeaff_fffe,
        address,
        value,
    ]);
    let mut full = vec![0xff; SIZE];
    full[..image.len()].copy_from_slice(image);
    bus.load_save_image(&full).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    cpu.step(&mut bus).unwrap();
    cpu.step(&mut bus).unwrap();
    Machine::new(cpu, bus)
}

// Aligned results match hardware expectations published with the mGBA test suite
// (memory suite, "SRAM store"/"SRAM load"). Unaligned stores were measured only as
// leaving byte 0 untouched; the stored lane follows the ARM7 data-bus layout.
#[test]
fn cpu_wide_stores_write_the_address_byte_lane() {
    const VALUE: u32 = 0xa5b6_c7d8;
    for (instruction, offset, byte) in [
        (0xe580_1000, 0, 0xd8), // STR r1,[r0]
        (0xe580_1000, 1, 0xc7),
        (0xe580_1000, 2, 0xb6),
        (0xe580_1000, 3, 0xa5),
        (0xe1c0_10b0, 0, 0xd8), // STRH r1,[r0]
        (0xe1c0_10b0, 1, 0xc7),
        (0xe1c0_10b0, 2, 0xd8),
        (0xe1c0_10b0, 3, 0xc7),
    ] {
        let mut machine = program_with(SAVE_START + offset, VALUE, instruction, &[]);
        machine.step().unwrap();
        let mut expected = [0xff; 4];
        expected[offset as usize] = byte;
        let image = machine.memory().save_image().unwrap();
        assert_eq!(&image[..4], &expected, "{instruction:#010x} +{offset}");
        assert!(machine.memory().save_modified());
    }
}

#[test]
fn cpu_wide_loads_repeat_the_addressed_byte_then_rotate() {
    let image = *b"Game";
    for (instruction, offset, value) in [
        (0xe590_2000, 0, 0x4747_4747), // LDR r2,[r0]
        (0xe590_2000, 1, 0x6161_6161),
        (0xe590_2000, 3, 0x6565_6565),
        (0xe1d0_20b0, 0, 0x4747), // LDRH r2,[r0]
        (0xe1d0_20b0, 1, 0x6100_0061),
        (0xe1d0_20f0, 0, 0x4747), // LDRSH r2,[r0]
        (0xe1d0_20f0, 1, 0x61),   // Odd LDRSH loads a signed byte.
    ] {
        let mut machine = program_with(SAVE_START + offset, 0, instruction, &image);
        machine.step().unwrap();
        assert_eq!(
            machine.cpu().registers()[2],
            value,
            "{instruction:#010x} +{offset}"
        );
    }
    let mut machine = program_with(SAVE_START, 0, 0xe1d0_20f0, &[0x80]);
    machine.step().unwrap();
    assert_eq!(machine.cpu().registers()[2], 0xffff_8080);
}

#[test]
fn thumb_wide_accesses_use_the_same_byte_lanes() {
    // ADD r2,pc,#1; BX r2 enters Thumb at ROM+8, then STRH r1,[r0,#0]; LDR r3,[r0,#0].
    let mut bus = memory(&[
        0xe59f_0010,
        0xe59f_1010,
        0xe28f_2001,
        0xe12f_ff12,
        0x6803_8001,
        0xe7fe_e7fe,
        SAVE_START + 1,
        0xa5b6_c7d8,
    ]);
    let mut cpu = Cpu::new(ROM_START);
    for _ in 0..4 {
        cpu.step(&mut bus).unwrap();
    }
    let mut machine = Machine::new(cpu, bus);
    machine.step().unwrap();
    assert_eq!(&machine.memory().save_image().unwrap()[..2], &[0xff, 0xc7]);
    machine.step().unwrap();
    assert_eq!(machine.cpu().registers()[3], 0xc7c7_c7c7);
}

#[test]
fn dma_into_sram_remains_a_diagnostic() {
    let mut bus = memory(&[0xeaff_fffe]);
    bus.write32(DMA_BASE + 0x24, 0x0300_0000).unwrap(); // DMA3 source
    bus.write32(DMA_BASE + 0x28, SAVE_START).unwrap(); // DMA3 destination
    bus.write32(DMA_BASE + 0x2c, 0x8000_0001).unwrap(); // One halfword, immediate
    let mut machine = Machine::new(Cpu::new(ROM_START), bus);
    assert!(matches!(machine.step(), Err(MachineError::Dma(_))));
    assert!(!machine.memory().save_modified());
}

#[test]
fn save_states_restore_sram_and_reject_other_save_devices() {
    let mut machine = program(SAVE_START + 1, 0xe5c0_0000);
    machine.step().unwrap();
    let state = machine.save_state().unwrap();
    machine.memory_mut().write8(SAVE_START + 1, 0x55).unwrap();
    machine.load_state(&state).unwrap();
    assert_eq!(machine.memory().read8(SAVE_START + 1).unwrap(), 0x01);
    assert_eq!(machine.save_state().unwrap(), state);

    let mut flash = program(SAVE_START + 1, 0xe5c0_0000);
    flash.memory_mut().set_save_device(SaveDevice::Flash64);
    assert_eq!(
        flash.load_state(&state),
        Err(SnapshotError::ConfigMismatch("cartridge setup"))
    );
}
