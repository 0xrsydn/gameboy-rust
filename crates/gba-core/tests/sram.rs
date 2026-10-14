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
fn halfword_and_word_accesses_remain_diagnostics() {
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

#[test]
fn failed_halfword_store_changes_nothing() {
    let mut machine = program(SAVE_START, 0xe1c0_00b0); // STRH r0,[r0]
    let before = machine.cpu().clone();
    assert!(machine.step().is_err());
    assert_eq!(machine.cpu(), &before);
    assert!(!machine.memory().save_modified());
    assert!(!machine.memory().save_write_pending());
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
