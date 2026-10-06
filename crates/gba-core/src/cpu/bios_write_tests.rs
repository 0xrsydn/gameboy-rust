//! Original BIOS-bus write probes. Images stay immutable; bus transfers still complete.
use super::*;
use crate::{
    dma::{DmaError, DMA_BASE},
    io::{IF, TIMER_BASE},
    machine::{Machine, MachineError, StepKind},
    memory::{BIOS_SIZE, ROM_START},
};

const DATA: u32 = 0x9234_abcd;
const RAM: u32 = 0x0200_0100;

fn setup(pc: u32, instruction: u32, thumb: bool) -> (Cpu, Memory, Vec<u8>) {
    let mut image = vec![0x55; BIOS_SIZE];
    let rom = [instruction, 0xe1a0_0000, 0xe1a0_0000]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    if pc < BIOS_SIZE as u32 {
        image[pc as usize..pc as usize + 4].copy_from_slice(&instruction.to_le_bytes());
    }
    let mut memory = Memory::with_bios(rom, image.clone()).unwrap();
    if pc == RAM {
        memory.write32(pc, instruction).unwrap();
        memory.write32(pc + 4, 0xe1a0_0000).unwrap();
        memory.write32(pc + 8, 0xe1a0_0000).unwrap();
    }
    let mut cpu = Cpu::new(pc);
    if thumb {
        cpu.instruction_set = InstructionSet::Thumb;
    }
    cpu.registers[1] = DATA;
    (cpu, memory, image)
}

fn unchanged(memory: &Memory, image: &[u8]) {
    for (address, expected) in image.iter().enumerate() {
        assert_eq!(memory.read8(address as u32).unwrap(), *expected);
    }
}

#[test]
fn bios_stores_ignore_data_in_both_states_all_modes_widths_and_address_lanes() {
    for (arm, thumb) in [
        (0xe5c0_1000, 0x7001),
        (0xe1c0_10b0, 0x8001),
        (0xe580_1000, 0x6001),
    ] {
        for is_thumb in [false, true] {
            for pc in [0x100, RAM, ROM_START] {
                for mode in [
                    Mode::User,
                    Mode::System,
                    Mode::Supervisor,
                    Mode::Irq,
                    Mode::Fiq,
                    Mode::Abort,
                    Mode::Undefined,
                ] {
                    for address in [0, 1, 2, 3, 0x3ffc, 0x3ffd, 0x3ffe, 0x3fff] {
                        let (mut cpu, mut memory, image) =
                            setup(pc, if is_thumb { thumb } else { arm }, is_thumb);
                        cpu.apply_status(
                            0xb000_00c0 | (u32::from(is_thumb) << 5) | mode as u32,
                            mode,
                        );
                        cpu.registers[0] = address;
                        let mut expected = cpu.clone();
                        expected.registers[15] += if is_thumb { 2 } else { 4 };
                        let timing = cpu.step_timed(&mut memory).unwrap();
                        assert_cpu_arch_eq!(cpu, expected);
                        assert_eq!(timing.data_cycles, 1);
                        assert_eq!(memory.cycles(), 0); // CPU-only stepping never clocks devices.
                        unchanged(&memory, &image);
                        assert_eq!(memory.write8(0, 1), Err(MemoryError::ReadOnly(0)));
                    }
                }
            }
        }
    }
}

#[test]
fn ignored_block_stores_write_back_but_a_later_unmapped_word_rolls_back_the_instruction() {
    for thumb in [false, true] {
        for base in [0x3ff8, 0x3ffc] {
            let (mut cpu, mut memory, image) =
                setup(ROM_START, if thumb { 0xc006 } else { 0xe8a0_0006 }, thumb);
            cpu.registers[0] = base;
            cpu.registers[2] = !DATA;
            let before = cpu.clone();
            let result = cpu.step_timed(&mut memory);
            if base == 0x3ff8 {
                assert_eq!(result.unwrap().data_cycles, 2);
                assert_eq!(cpu.registers[0], base + 8);
                assert_eq!(cpu.pc(), ROM_START + if thumb { 2 } else { 4 });
            } else {
                assert_eq!(result, Err(CpuError::Memory(MemoryError::Unmapped(0x4000))));
                assert_eq!(cpu, before);
                assert_eq!(cpu.step_timed(&mut memory), result);
            }
            unchanged(&memory, &image);
        }
    }
}

#[test]
fn swap_keeps_protected_read_lanes_and_does_not_latch_the_ignored_write_value() {
    for byte in [false, true] {
        for lane in 0..4 {
            let instruction = if byte { 0xe140_2091_u32 } else { 0xe100_2091 }; // SWPB/SWP r2,r1,[r0]
            let rom = [instruction, 0xe590_3000, 0xe1a0_0000, 0xe1a0_0000]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect();
            // Enter BIOS using a synthetic image to establish known protected read history.
            let mut image = vec![0x55; BIOS_SIZE];
            image[0x100..0x104].copy_from_slice(&0xe12f_ff14_u32.to_le_bytes()); // BX r4
            image[0x108..0x10c].copy_from_slice(&0x8765_4321_u32.to_le_bytes());
            let mut memory = Memory::with_bios(rom, image.clone()).unwrap();
            let mut cpu = Cpu::new(0x100);
            cpu.registers[4] = ROM_START;
            cpu.step(&mut memory).unwrap();
            cpu.registers[0] = lane;
            cpu.registers[1] = DATA;
            assert_eq!(cpu.step_timed(&mut memory).unwrap().data_cycles, 2);
            let rotated = 0x8765_4321_u32.rotate_right(lane * 8);
            assert_eq!(cpu.registers[2], if byte { rotated & 255 } else { rotated });
            cpu.step(&mut memory).unwrap(); // Protected LDR r3,[r0] retains the old BIOS fetch.
            assert_eq!(cpu.registers[3], rotated);
            unchanged(&memory, &image);
        }
    }
}

#[test]
fn swap_with_unknown_bios_read_history_still_fails_before_the_write() {
    let (mut cpu, mut memory, image) = setup(ROM_START, 0xe100_2091, false);
    cpu.registers[0] = 0;
    let before = cpu.clone();
    assert_eq!(
        cpu.step(&mut memory),
        Err(CpuError::Memory(MemoryError::Unmapped(0)))
    );
    assert_eq!(cpu, before);
    unchanged(&memory, &image);
}

#[test]
fn ignored_cpu_stores_still_advance_timers_and_preserve_strict_host_diagnostics() {
    let (mut cpu, mut memory, image) = setup(ROM_START, 0xe480_1004, false); // STR r1,[r0],#4
    cpu.registers[0] = 0x200;
    memory.write32(TIMER_BASE, 0x0080_0000).unwrap();
    let mut machine = Machine::new(cpu, memory);
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    assert_eq!(machine.cpu().registers()[0], 0x204);
    assert_eq!(machine.last_timing().data_cycles, 1);
    assert_eq!(u64::from(machine.last_timing().total()), machine.cycles());
    assert_eq!(
        u64::from(machine.memory().read16(TIMER_BASE).unwrap()),
        machine.cycles()
    );
    unchanged(machine.memory(), &image);
    for address in [0, 0x3ffc] {
        assert_eq!(
            machine.memory_mut().write8(address, 0),
            Err(MemoryError::ReadOnly(address))
        );
        assert_eq!(
            machine.memory_mut().write16(address, 0),
            Err(MemoryError::ReadOnly(address))
        );
        assert_eq!(
            machine.memory_mut().write32(address, 0),
            Err(MemoryError::ReadOnly(address))
        );
    }
}

#[test]
fn unmapped_bios_and_cartridge_destinations_remain_retryable_diagnostics() {
    for (address, mapped_bios) in [
        (0, false),
        (0x4000, true),
        (0x0100_0000, true),
        (ROM_START, true),
    ] {
        let (mut cpu, mut memory, _) = setup(ROM_START, 0xe480_1004, false);
        if !mapped_bios {
            memory = Memory::new(
                [0xe480_1004_u32, 0xe1a0_0000, 0xe1a0_0000]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect(),
            )
            .unwrap();
        }
        cpu.registers[0] = address;
        let before = cpu.clone();
        let mut machine = Machine::new(cpu, memory);
        let error = machine.step().unwrap_err();
        assert_eq!(
            error,
            MachineError::Cpu(CpuError::Memory(if address == ROM_START {
                MemoryError::ReadOnly(address)
            } else {
                MemoryError::Unmapped(address)
            }))
        );
        assert_eq!(machine.cpu(), &before);
        assert_eq!(machine.cycles(), 0);
        assert_eq!(machine.step(), Err(error));
    }
}

#[test]
fn dma_ignored_writes_pay_cycles_complete_and_raise_the_normal_completion_irq() {
    // DMA0's destination mask turns ROM_START into BIOS address zero.
    for (word, destination) in [(false, 0), (true, 0), (false, ROM_START), (true, ROM_START)] {
        let (cpu, mut memory, image) = setup(ROM_START, 0xeaff_fffe, false);
        memory.write32(RAM, DATA).unwrap();
        memory.write32(DMA_BASE, RAM).unwrap();
        memory.write32(DMA_BASE + 4, destination).unwrap();
        memory
            .write32(DMA_BASE + 8, if word { 0xc400_0001 } else { 0xc000_0001 })
            .unwrap();
        memory.write32(TIMER_BASE, 0x0080_0000).unwrap();
        let before = cpu.clone();
        let mut machine = Machine::new(cpu, memory);
        assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 0 });
        assert_eq!(machine.cpu(), &before);
        assert_eq!(machine.cycles(), if word { 9 } else { 6 }); // Startup + EWRAM read + BIOS write.
        assert_eq!(
            u64::from(machine.memory().read16(TIMER_BASE).unwrap()),
            machine.cycles()
        );
        assert_eq!(machine.memory().read16(IF).unwrap(), 0x100);
        assert_eq!(machine.memory().read16(DMA_BASE + 10).unwrap() & 0x8000, 0);
        unchanged(machine.memory(), &image);
        // A blocked source now reuses this completed unit's data, not BIOS bytes.
        machine.memory_mut().write32(DMA_BASE, 0).unwrap();
        machine.memory_mut().write32(DMA_BASE + 4, RAM + 8).unwrap();
        machine
            .memory_mut()
            .write32(DMA_BASE + 8, 0x8400_0001)
            .unwrap();
        machine.step().unwrap();
        assert_eq!(
            machine.memory().read32(RAM + 8).unwrap(),
            if word { DATA } else { 0xabcd_abcd }
        );
    }
}

#[test]
fn dma_bios_boundary_keeps_completed_units_and_retries_the_unmapped_destination() {
    let (cpu, mut memory, image) = setup(ROM_START, 0xeaff_fffe, false);
    memory.write32(RAM, DATA).unwrap();
    memory.write32(DMA_BASE, RAM).unwrap();
    memory.write32(DMA_BASE + 4, 0x3ffc).unwrap();
    memory.write32(DMA_BASE + 8, 0xc400_0002).unwrap();
    let mut machine = Machine::new(cpu, memory);
    machine.step().unwrap();
    let cycles = machine.cycles();
    let timing = machine.last_timing();
    let error = MachineError::Dma(DmaError::Memory {
        channel: 0,
        error: MemoryError::Unmapped(0x4000),
    });
    assert_eq!(machine.step(), Err(error.clone()));
    assert_eq!(machine.step(), Err(error));
    assert_eq!(machine.cycles(), cycles);
    assert_eq!(machine.last_timing(), timing);
    assert_eq!(machine.memory().read16(IF).unwrap(), 0);
    unchanged(machine.memory(), &image);
}
