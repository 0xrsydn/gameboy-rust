//! Original Game Pak ROM reads beyond the supplied image; no game assets.
use gba_core::{
    cpu::{Cpu, CpuError},
    dma::DMA_BASE,
    machine::{Machine, MachineError, StepKind},
    memory::{Memory, MemoryError, ROM_START},
};

fn words(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

/// LDR r0,=address; then `load`; then loop. The image is 16 bytes, a whole chip.
fn load(address: u32, load: u32) -> Machine {
    let mut bus = Memory::new(words(&[0xe59f_0004, load, 0xeaff_fffe, address])).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    cpu.step(&mut bus).unwrap();
    Machine::new(cpu, bus)
}

#[test]
fn cpu_reads_beyond_the_chip_return_the_halfword_address_in_every_rom_window() {
    for (address, instruction, expected) in [
        (0x0800_1234, 0xe590_1000, 0x091b_091a), // LDR: two halfword addresses.
        (0x09fe_20f8, 0xe1d0_10b0, 0x107c),      // LDRH: past a 16 MiB image.
        (0x0800_1235, 0xe5d0_1000, 0x09),        // LDRB of the high lane.
        (0x0a00_1234, 0xe590_1000, 0x091b_091a), // Wait-state 1 window.
        (0x0c00_1234, 0xe590_1000, 0x091b_091a), // Wait-state 2 window.
    ] {
        let mut machine = load(address, instruction);
        assert_eq!(machine.step().unwrap(), StepKind::Instruction);
        assert_eq!(machine.cpu().registers()[1], expected, "{address:#010x}");
    }
}

#[test]
fn reads_past_a_trimmed_image_take_the_bus_pattern_unless_padding_is_strict() {
    // A 12-byte image in a 16-byte chip. By default offset 12 (lost padding) and 16 (beyond
    // the chip) both read the halfword-address pattern, as in mGBA; strict padding reports
    // offset 12 instead, a debugging aid for truncated dumps.
    for (strict, target, expected) in [
        (false, ROM_START + 12, Ok(0x0007_0006)),
        (false, ROM_START + 16, Ok(0x0009_0008)),
        (
            true,
            ROM_START + 12,
            Err(MemoryError::Unmapped(ROM_START + 12)),
        ),
        (true, ROM_START + 16, Ok(0x0009_0008)),
    ] {
        // LDR r0,[pc,#0] (the literal at offset 8); LDR r1,[r0]; literal.
        let mut bus = Memory::new(words(&[0xe59f_0000, 0xe590_1000, target])).unwrap();
        bus.set_strict_rom_padding(strict);
        let mut cpu = Cpu::new(ROM_START);
        cpu.step(&mut bus).unwrap();
        let before = cpu.clone();
        match expected {
            Ok(value) => {
                cpu.step(&mut bus).unwrap();
                assert_eq!(cpu.registers()[1], value, "{strict} {target:#x}");
            }
            Err(error) => {
                assert_eq!(cpu.step(&mut bus), Err(CpuError::Memory(error)));
                assert_eq!(cpu, before);
            }
        }
    }
    let bus = Memory::new(words(&[0, 0, 0])).unwrap();
    assert_eq!(
        bus.read32(ROM_START + 12),
        Err(MemoryError::Unmapped(ROM_START + 12)),
        "host reads stay strict"
    );
}

#[test]
fn without_a_save_chip_the_save_region_reads_0xff_and_ignores_stores() {
    // STRB r1,[r0]; LDRB r2,[r0]; LDR r3,[r0]; B . (r0 = 0x0e000001, as Another World reads)
    for address in [0x0e00_0001, 0x0fff_fff0] {
        let code = [
            0xe59f_0010, // LDR r0,[pc,#16]
            0xe3a0_1012, // MOV r1,#0x12
            0xe5c0_1000, // STRB r1,[r0]
            0xe5d0_2000, // LDRB r2,[r0]
            0xe590_3000, // LDR r3,[r0]
            0xeaff_fffe, // B .
            address,
        ];
        let mut bus = Memory::new(words(&code)).unwrap();
        let mut cpu = Cpu::new(ROM_START);
        for _ in 0..5 {
            cpu.step(&mut bus).unwrap();
        }
        assert_eq!(cpu.registers()[2], 0xff, "{address:#x}");
        assert_eq!(cpu.registers()[3], 0xffff_ffff, "{address:#x}");
        assert_eq!(
            bus.read8(address),
            Err(MemoryError::Unmapped(address)),
            "host stays strict"
        );
    }
}

#[test]
fn host_reads_and_instruction_fetches_beyond_the_chip_stay_strict() {
    let bus = Memory::new(words(&[0xeaff_fffe])).unwrap();
    assert_eq!(
        bus.read16(ROM_START + 4),
        Err(MemoryError::Unmapped(ROM_START + 4))
    );
    let mut machine = Machine::new(Cpu::new(ROM_START + 0x100), bus);
    assert!(matches!(machine.step(), Err(MachineError::Cpu(_))));
}

#[test]
fn dma_reads_beyond_the_chip_return_the_halfword_address() {
    let mut bus = Memory::new(words(&[0xeaff_fffe])).unwrap();
    bus.write32(DMA_BASE + 0x24, 0x0800_1234).unwrap(); // DMA3 source
    bus.write32(DMA_BASE + 0x28, 0x0300_0000).unwrap(); // DMA3 destination
    bus.write32(DMA_BASE + 0x2c, 0x8400_0001).unwrap(); // One word, immediate
    let mut machine = Machine::new(Cpu::new(ROM_START), bus);
    assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 3 });
    assert_eq!(machine.memory().read32(0x0300_0000).unwrap(), 0x091b_091a);
}
