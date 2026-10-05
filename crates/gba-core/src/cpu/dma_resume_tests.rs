//! Original tests of the nominal, instruction-boundary DMA resume cost.
use super::*;
use crate::{
    dma::{DMA_BASE, DMA_STRIDE},
    input::Buttons,
    io::{HALTCNT, IE, IME, KEYCNT, TIMER_BASE, WAITCNT},
    machine::{Machine, StepKind},
    memory::{BIOS_SIZE, ROM_START},
};

const RAM: u32 = 0x0200_1000;
const DEST: u32 = 0x0300_1000;
const PC: u32 = ROM_START + 0x100;

fn program(thumb: bool, pc: u32, instruction: u32) -> (Cpu, Memory) {
    let nop = if thumb { 0x46c0_46c0_u32 } else { 0xe1a0_0000 };
    let offset = (pc & 0x01ff_ffff) as usize;
    let mut rom = nop.to_le_bytes().repeat((offset + 32) / 4);
    let width = if thumb { 2 } else { 4 };
    rom[offset..offset + width].copy_from_slice(&instruction.to_le_bytes()[..width]);
    let mut cpu = Cpu::new(pc);
    if thumb {
        cpu.instruction_set = InstructionSet::Thumb;
    }
    cpu.registers[1] = RAM;
    (cpu, Memory::new(rom).unwrap())
}

fn configure(bus: &mut Memory, channel: usize, source: u32, destination: u32, count: u16) {
    let base = DMA_BASE + channel as u32 * DMA_STRIDE;
    bus.write16(base + 10, 0).unwrap();
    bus.write32(base, source).unwrap();
    bus.write32(base + 4, destination).unwrap();
    bus.write32(base + 8, 0x8400_0000 | u32::from(count))
        .unwrap();
}

fn dma(bus: &mut Memory) {
    configure(bus, 3, RAM, DEST, 1);
    assert_eq!(bus.step_dma().unwrap().unwrap().0, 3);
}

#[test]
fn first_post_dma_code_access_uses_n_then_s_for_all_rom_wait_settings() {
    for thumb in [false, true] {
        for window in 0..3 {
            for first in 0..4 {
                for second in 0..2 {
                    for prefetch in [0, 0x4000] {
                        let pc = PC + window as u32 * 0x0200_0000;
                        let instruction = if thumb { 0x46c0 } else { 0xe1a0_0000 };
                        let (mut cpu, mut bus) = program(thumb, pc, instruction);
                        let waitcnt = (first << [2, 5, 8][window])
                            | (second << [4, 7, 10][window])
                            | prefetch;
                        bus.write16(WAITCNT, waitcnt).unwrap();
                        dma(&mut bus);
                        // Independent table, not the production timing helper.
                        let n = [5, 4, 3, 9][first as usize];
                        let s = if second == 1 { 2 } else { [3, 5, 9][window] };
                        assert_eq!(
                            cpu.step_timed(&mut bus).unwrap().code_cycles,
                            n + if thumb { 0 } else { s }
                        );
                        assert_eq!(
                            cpu.step_timed(&mut bus).unwrap().code_cycles,
                            if thumb { s } else { 2 * s }
                        );
                        assert_eq!(bus.cycles(), 9); // CPU-only timed calls do not advance devices.
                    }
                }
            }
        }
    }
}

#[test]
fn ram_and_rom_dma_sources_both_break_the_cpu_sequence() {
    for source in [RAM, DEST, ROM_START] {
        let (cpu, mut bus) = program(false, PC, 0xe1a0_0000);
        configure(&mut bus, 3, source, DEST + 16, 1);
        let mut machine = Machine::new(cpu, bus);
        assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 3 });
        assert_eq!(machine.step().unwrap(), StepKind::Instruction);
        assert_eq!(machine.last_timing().code_cycles, 8);
        machine.step().unwrap();
        assert_eq!(machine.last_timing().code_cycles, 6);
    }
}

#[test]
fn register_configuration_alone_is_not_a_dma_resume_but_blocked_source_completion_is() {
    let (mut cpu, mut bus) = program(false, PC, 0xe1a0_0000);
    configure(&mut bus, 3, RAM, DEST, 1);
    // CPU-only execution does not service the configured transfer.
    assert_eq!(cpu.step_timed(&mut bus).unwrap().code_cycles, 6);
    bus.step_dma().unwrap().unwrap();
    assert_eq!(cpu.step_timed(&mut bus).unwrap().code_cycles, 8);
    configure(&mut bus, 3, 0, DEST, 1); // Known channel data, no BIOS read.
    bus.step_dma().unwrap().unwrap();
    assert_eq!(cpu.step_timed(&mut bus).unwrap().code_cycles, 8);
    assert_eq!(cpu.step_timed(&mut bus).unwrap().code_cycles, 6);
}

#[test]
fn multiple_units_and_channel_preemption_produce_one_resume_not_a_penalty_queue() {
    let (cpu, mut bus) = program(false, PC, 0xe1a0_0000);
    configure(&mut bus, 3, RAM, DEST, 2);
    let mut machine = Machine::new(cpu, bus);
    machine.step().unwrap();
    configure(machine.memory_mut(), 0, RAM + 16, DEST + 16, 1);
    for channel in [0, 3] {
        assert_eq!(machine.step().unwrap(), StepKind::Dma { channel });
    }
    machine.step().unwrap();
    assert_eq!(machine.last_timing().code_cycles, 8);
    machine.step().unwrap();
    assert_eq!(machine.last_timing().code_cycles, 6);
    // Another block after CPU progress must mark the next resume again.
    configure(machine.memory_mut(), 3, RAM, DEST, 1);
    machine.step().unwrap();
    machine.step().unwrap();
    assert_eq!(machine.last_timing().code_cycles, 8);
}

#[test]
fn failed_dma_preserves_an_existing_resume_but_cannot_create_one() {
    for completed in [false, true] {
        let (mut cpu, mut bus) = program(false, PC, 0xe1a0_0000);
        if completed {
            dma(&mut bus);
        }
        configure(&mut bus, 3, 0x0e00_0000, DEST, 1);
        let cycles = bus.cycles();
        for _ in 0..2 {
            assert!(bus.step_dma().is_err());
            assert_eq!(bus.cycles(), cycles);
        }
        bus.write16(DMA_BASE + 3 * DMA_STRIDE + 10, 0).unwrap();
        assert_eq!(
            cpu.step_timed(&mut bus).unwrap().code_cycles,
            if completed { 8 } else { 6 }
        );
    }
}

#[test]
fn failed_cpu_fetch_and_data_access_preserve_resume_until_success() {
    let (mut cpu, mut bus) = program(false, PC, 0xe591_0000); // LDR r0,[r1].
    dma(&mut bus);
    cpu.registers[15] = 0x0e00_0000;
    assert!(cpu.step_timed(&mut bus).is_err());
    bus.write32(DEST + 0x100, 0xffff_ffff).unwrap(); // Unsupported ARM encoding.
    cpu.registers[15] = DEST + 0x100;
    assert!(cpu.step_timed(&mut bus).is_err());
    cpu.registers[15] = PC;
    cpu.registers[1] = 0x0e00_0000;
    let before = cpu.clone();
    for _ in 0..2 {
        assert!(cpu.step_timed(&mut bus).is_err());
        assert_eq!(cpu, before);
    }
    cpu.registers[1] = RAM;
    let timing = cpu.step_timed(&mut bus).unwrap();
    assert_eq!(timing.code_cycles, 8);
    assert_eq!(timing.data_cycles, 6);
    assert_eq!(timing.internal_cycles, 1);
    assert_eq!(cpu.step_timed(&mut bus).unwrap().code_cycles, 6);
}

#[test]
fn host_accesses_and_clock_advances_do_not_consume_resume_but_untimed_cpu_does() {
    for untimed in [false, true] {
        let (mut cpu, mut bus) = program(false, PC, 0xe1a0_0000);
        dma(&mut bus);
        bus.read32(PC).unwrap();
        bus.read32(RAM).unwrap();
        bus.write32(DEST, 0).unwrap();
        bus.advance_cycles(10);
        if untimed {
            cpu.step(&mut bus).unwrap();
        }
        assert_eq!(
            cpu.step_timed(&mut bus).unwrap().code_cycles,
            if untimed { 6 } else { 8 }
        );
    }
}

#[test]
fn skipped_conditions_stores_and_boundaries_consume_resume_without_double_charging() {
    for (pc, instruction, data) in [
        (PC, 0x0591_0000, 0),                  // LDREQ skipped: Z is clear.
        (PC, 0xe581_0000, 6),                  // STR already has an N code access.
        (ROM_START + 0x20000, 0xe1a0_0000, 0), // Boundary already forces N.
    ] {
        let (mut cpu, mut bus) = program(false, pc, instruction);
        dma(&mut bus);
        let timing = cpu.step_timed(&mut bus).unwrap();
        assert_eq!(timing.code_cycles, 8);
        assert_eq!(timing.data_cycles, data);
        assert_eq!(cpu.step_timed(&mut bus).unwrap().code_cycles, 6);
    }
}

#[test]
fn waitcnt_cpu_store_uses_old_code_cost_but_dma_store_applies_before_resume() {
    let (mut cpu, mut bus) = program(false, PC, 0xe581_0000);
    cpu.registers[0] = 0x18;
    cpu.registers[1] = WAITCNT;
    dma(&mut bus);
    assert_eq!(cpu.step_timed(&mut bus).unwrap().code_cycles, 8);
    assert_eq!(cpu.step_timed(&mut bus).unwrap().code_cycles, 4);

    let (mut cpu, mut bus) = program(false, PC, 0xe1a0_0000);
    bus.write32(RAM, 0x18).unwrap();
    configure(&mut bus, 3, RAM, WAITCNT, 1);
    bus.step_dma().unwrap().unwrap();
    assert_eq!(cpu.step_timed(&mut bus).unwrap().code_cycles, 5); // N=3, S=2.
    assert_eq!(cpu.step_timed(&mut bus).unwrap().code_cycles, 4);
}

#[test]
fn ram_instruction_consumes_resume_instead_of_delaying_it_until_rom_execution() {
    let (mut cpu, mut bus) = program(false, PC, 0xe1a0_0000);
    bus.write32(DEST + 0x100, 0xe1a0_0000).unwrap();
    cpu.registers[15] = DEST + 0x100;
    dma(&mut bus);
    assert_eq!(cpu.step_timed(&mut bus).unwrap().code_cycles, 1);
    cpu.registers[15] = PC;
    assert_eq!(cpu.step_timed(&mut bus).unwrap().code_cycles, 6);
}

#[test]
fn branch_refill_and_irq_entry_consume_resume_without_an_extra_nominal_access() {
    let (mut cpu, mut bus) = program(false, PC, 0xea00_0000); // B PC+8.
    dma(&mut bus);
    assert_eq!(cpu.step_timed(&mut bus).unwrap().code_cycles, 20); // Existing N+2S summary.
    assert_eq!(cpu.step_timed(&mut bus).unwrap().code_cycles, 6);

    let (cpu, _) = program(false, PC, 0xe1a0_0000);
    let mut bios = vec![0; BIOS_SIZE];
    bios[0x18..0x1c].copy_from_slice(&0xe1a0_0000_u32.to_le_bytes());
    let mut bus = Memory::with_bios(0xe1a0_0000_u32.to_le_bytes().repeat(80), bios).unwrap();
    configure(&mut bus, 3, RAM, DEST, 1);
    bus.write16(DMA_BASE + 3 * DMA_STRIDE + 10, 0xc400).unwrap();
    bus.write16(IE, 1 << 11).unwrap();
    bus.write16(IME, 1).unwrap();
    let mut machine = Machine::new(cpu, bus);
    assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 3 });
    assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
    assert_eq!(machine.last_timing().code_cycles, 3);
    // Inspect the bus through a separate CPU after IRQ entry, before a handler runs.
    let mut probe = Cpu::new(PC);
    assert_eq!(
        probe.step_timed(machine.memory_mut()).unwrap().code_cycles,
        6
    );
    assert_eq!(machine.memory().cycles(), 12);
}

#[test]
fn halt_and_stop_idle_do_not_consume_a_completed_dma_resume() {
    for stop in [false, true] {
        let (cpu, mut bus) = program(false, PC, 0xe1a0_0000);
        dma(&mut bus);
        let mut machine = Machine::new(cpu, bus);
        if stop {
            machine.memory_mut().write16(KEYCNT, 0x4001).unwrap();
            machine.memory_mut().write16(IE, 1 << 12).unwrap();
            machine.memory_mut().write8(HALTCNT, 0x80).unwrap();
            assert_eq!(machine.step().unwrap(), StepKind::StopIdle);
            machine.memory_mut().set_buttons(Buttons::from_bits(1));
        } else {
            machine.memory_mut().write16(IE, 8).unwrap();
            machine
                .memory_mut()
                .write32(TIMER_BASE, 0x00c0_fff0)
                .unwrap();
            machine.memory_mut().write8(HALTCNT, 0).unwrap();
            assert_eq!(machine.step().unwrap(), StepKind::HaltIdle);
        }
        assert_eq!(machine.step().unwrap(), StepKind::Instruction);
        assert_eq!(machine.last_timing().code_cycles, 8);
        machine.step().unwrap();
        assert_eq!(machine.last_timing().code_cycles, 6);
    }
}

#[test]
fn resumed_n_cost_advances_devices_and_defers_irq_to_the_following_step() {
    let (cpu, mut bus) = program(false, PC, 0xe1a0_0000);
    dma(&mut bus);
    bus.write32(TIMER_BASE, 0x00c0_fff9).unwrap(); // Overflow after seven cycles.
    bus.write16(IE, 8).unwrap();
    bus.write16(IME, 1).unwrap();
    let mut machine = Machine::new(cpu, bus);
    let start = machine.cycles();
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    assert_eq!(machine.cycles() - start, 8);
    assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
}
