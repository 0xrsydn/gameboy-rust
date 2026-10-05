//! Original instruction-boundary DMA/IWRAM history tests, not sub-instruction arbitration.
use super::*;
use crate::{
    dma::{DMA_BASE, DMA_STRIDE},
    io::{IE, IME, TIMER_BASE},
    machine::{Machine, StepKind},
};

const CODE: u32 = 0x0300_0100;
const IWRAM: u32 = 0x0300_1000;
const EWRAM: u32 = 0x0200_1000;
const SEED: u32 = 0x1234_abcd;
const DATA: u32 = 0x97e6_b580;
const NEXT: u16 = 0xd238;
const PROBE: u16 = 0x6806; // LDR r6,[r0]
const UNUSED: u32 = 0x8000_0000;

fn program(pc: u32, first: u16) -> (Cpu, Memory) {
    let mut bus = Memory::new(vec![]).unwrap();
    for (i, half) in [first, PROBE, 0x4321, NEXT].into_iter().enumerate() {
        bus.write16(pc + i as u32 * 2, half).unwrap();
    }
    bus.write32(IWRAM, SEED).unwrap();
    let mut cpu = Cpu::new(pc);
    cpu.instruction_set = InstructionSet::Thumb;
    cpu.registers[0] = UNUSED;
    cpu.registers[1] = IWRAM;
    (cpu, bus)
}

fn ready(pc: u32) -> Machine {
    let (cpu, bus) = program(pc, 0x680a); // LDR r2,[r1] establishes both lanes.
    let mut machine = Machine::new(cpu, bus);
    machine.step().unwrap();
    machine
}

fn configure(
    bus: &mut Memory,
    channel: usize,
    source: u32,
    destination: u32,
    count: u16,
    word: bool,
) {
    let base = DMA_BASE + channel as u32 * DMA_STRIDE;
    bus.write16(base + 10, 0).unwrap(); // Disable before rearming.
    bus.write32(base, source).unwrap();
    bus.write32(base + 4, destination).unwrap();
    bus.write32(
        base + 8,
        (if word { 0x8400_0000 } else { 0x8000_0000 }) | u32::from(count),
    )
    .unwrap();
}

fn transfer(machine: &mut Machine, source: u32, destination: u32, word: bool) {
    configure(machine.memory_mut(), 3, source, destination, 1, word);
    let cpu = machine.cpu().clone();
    assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 3 });
    assert_eq!(machine.cpu(), &cpu);
}

// Independent byte-array reference. Only page 3 drives the local IWRAM lanes.
fn drive(lanes: &mut [u8; 4], address: u32, bytes: &[u8]) {
    if address >> 24 == 3 {
        let start = (address & 3) as usize;
        lanes[start..start + bytes.len()].copy_from_slice(bytes);
    }
}

fn resumed(lanes: [u8; 4], pc: u32) -> u32 {
    let mut lanes = lanes;
    drive(&mut lanes, pc + 6, &NEXT.to_le_bytes());
    u32::from_le_bytes(lanes)
}

#[test]
fn dma_source_and_destination_drive_only_addressed_iwram_lanes_before_resume_fetch() {
    for pc in [CODE, CODE + 2, CODE + 0x8000, 0x0300_7ffc, 0x0300_7ffe] {
        for word in [false, true] {
            for source in [IWRAM + 0x8000, IWRAM + 0x8002, EWRAM, EWRAM + 2] {
                for destination in [IWRAM + 0x100, IWRAM + 0x102, EWRAM + 0x100, EWRAM + 0x102] {
                    let width = if word { 4 } else { 2 };
                    let source = source & !(width - 1);
                    let destination = destination & !(width - 1);
                    let mut machine = ready(pc);
                    machine.memory_mut().write32(source & !3, DATA).unwrap();
                    let bytes = DATA.to_le_bytes();
                    let lane = (source & 3) as usize;
                    let bytes = &bytes[lane..lane + width as usize];
                    let mut expected = SEED.to_le_bytes();
                    drive(&mut expected, source, bytes);
                    drive(&mut expected, destination, bytes);
                    transfer(&mut machine, source, destination, word);
                    // Host reads/writes after DMA cannot replace retained lanes.
                    machine.memory_mut().write32(source & !3, 0).unwrap();
                    machine.memory().read32(destination & !3).unwrap();
                    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
                    assert_eq!(machine.cpu().registers()[6], resumed(expected, pc));
                }
            }
        }
    }
}

#[test]
fn halfword_channel_duplication_is_not_a_full_iwram_bus_write() {
    let mut machine = ready(CODE);
    machine.memory_mut().write32(IWRAM, DATA).unwrap();
    // High source lane only: low IWRAM bus lane must retain SEED's low half.
    transfer(&mut machine, IWRAM + 2, EWRAM, false);
    machine.step().unwrap();
    assert_eq!(
        machine.cpu().registers()[6],
        (u32::from(NEXT) << 16) | (SEED & 0xffff)
    );
}

#[test]
fn blocked_source_updates_only_the_destination_lane_using_its_selected_channel_half() {
    for pc in [CODE, CODE + 2] {
        for destination in [IWRAM + 0x100, IWRAM + 0x102, EWRAM + 0x100] {
            let mut machine = ready(pc);
            machine.memory_mut().write32(EWRAM, DATA).unwrap();
            transfer(&mut machine, EWRAM, EWRAM + 8, true); // Seed channel without touching IWRAM.
            transfer(&mut machine, 2, destination, false);
            let half = (DATA >> ((destination & 2) * 8)) as u16;
            let mut expected = SEED.to_le_bytes();
            drive(&mut expected, destination, &half.to_le_bytes());
            machine.step().unwrap();
            assert_eq!(machine.cpu().registers()[6], resumed(expected, pc));
            assert_eq!(machine.memory().read16(destination).unwrap(), half);
        }
    }
}

#[test]
fn completed_units_and_priority_changes_use_access_order_not_channel_number() {
    let mut machine = ready(CODE);
    machine.memory_mut().write32(EWRAM, DATA).unwrap();
    machine
        .memory_mut()
        .write32(EWRAM + 4, 0x7612_8abc)
        .unwrap();
    machine
        .memory_mut()
        .write32(EWRAM + 8, 0xaabb_eeff)
        .unwrap();
    configure(machine.memory_mut(), 3, EWRAM, IWRAM, 2, true);
    assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 3 });
    configure(machine.memory_mut(), 0, EWRAM + 8, IWRAM + 8, 1, true);
    for channel in [0, 3] {
        assert_eq!(machine.step().unwrap(), StepKind::Dma { channel });
    }
    machine.step().unwrap();
    assert_eq!(
        machine.cpu().registers()[6],
        resumed(0x7612_8abc_u32.to_le_bytes(), CODE)
    );
}

#[test]
fn failed_dma_and_failed_resumed_instruction_preserve_last_completed_dma_history() {
    for fail_destination in [false, true] {
        let (mut cpu, bus) = program(CODE, 0x680a);
        cpu.registers[0] = 0x0e00_0000; // Buffered PROBE will fail on unsupported save memory.
        let mut machine = Machine::new(cpu, bus);
        machine.step().unwrap();
        machine.memory_mut().write32(EWRAM, DATA).unwrap();
        transfer(&mut machine, EWRAM, IWRAM, true);
        // A valid IWRAM source must not drive history if its destination is invalid.
        machine.memory_mut().write32(IWRAM, 0x1122_3344).unwrap();
        configure(
            machine.memory_mut(),
            3,
            if fail_destination { IWRAM } else { 0x0e00_0000 },
            if fail_destination { 0x0800_0000 } else { IWRAM },
            1,
            true,
        );
        let before = machine.cpu().clone();
        let cycles = machine.cycles();
        let timing = machine.last_timing();
        for _ in 0..2 {
            assert!(machine.step().is_err());
            assert_eq!(machine.cpu(), &before);
            assert_eq!(machine.cycles(), cycles);
            assert_eq!(machine.last_timing(), timing);
        }
        machine
            .memory_mut()
            .write16(DMA_BASE + 3 * DMA_STRIDE + 10, 0)
            .unwrap();
        assert!(machine.step().is_err());
        assert_eq!(machine.cpu(), &before);
        assert_eq!(machine.cycles(), cycles);
        assert_eq!(machine.last_timing(), timing);
        // Repair the data address, not the retained instruction or DMA history.
        let mut cpu = machine.cpu().clone();
        cpu.registers[0] = UNUSED;
        cpu.step_timed(machine.memory_mut()).unwrap();
        assert_eq!(cpu.registers()[6], resumed(DATA.to_le_bytes(), CODE));
    }
}

#[test]
fn partial_continuation_can_gain_known_lanes_but_cold_direct_entry_stays_unknown() {
    let (mut cpu, mut bus) = program(CODE, 0x46c0); // NOP fills only the low lane.
    cpu.step(&mut bus).unwrap();
    bus.write32(EWRAM, DATA).unwrap();
    configure(&mut bus, 3, EWRAM, IWRAM, 1, true);
    bus.step_dma().unwrap().unwrap();
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[6], resumed(DATA.to_le_bytes(), CODE));

    // No executed instruction or refill established an expected continuation PC.
    let (mut cpu, mut bus) = program(CODE, PROBE);
    bus.write32(EWRAM, DATA).unwrap();
    configure(&mut bus, 3, EWRAM, IWRAM, 1, true);
    bus.step_dma().unwrap().unwrap();
    assert_eq!(
        cpu.step(&mut bus),
        Err(CpuError::Memory(MemoryError::Unmapped(UNUSED)))
    );
}

#[test]
fn irq_and_pc_discontinuity_do_not_reuse_dma_updated_continuations() {
    for irq in [false, true] {
        let mut machine = ready(CODE);
        machine.memory_mut().write32(EWRAM, DATA).unwrap();
        transfer(&mut machine, EWRAM, IWRAM, true);
        let mut cpu = machine.cpu().clone();
        if irq {
            machine.memory_mut().write16(IE, 8).unwrap();
            machine.memory_mut().write16(IME, 1).unwrap();
            machine
                .memory_mut()
                .write32(TIMER_BASE, 0x00c0_ffff)
                .unwrap();
            machine.memory_mut().advance_cycles(1);
            assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
        } else {
            cpu.registers[15] = CODE + 0x100;
            machine.memory_mut().write16(cpu.pc(), PROBE).unwrap();
        }
        assert_eq!(
            cpu.step(machine.memory_mut()),
            Err(CpuError::Memory(MemoryError::Unmapped(UNUSED)))
        );
    }
}

#[test]
fn arm_and_other_thumb_regions_keep_their_fetch_snapshot_not_dma_data() {
    for thumb in [false, true] {
        for pc in [0x0200_0100, 0x0300_0100, 0x0700_0100] {
            if thumb && pc >> 24 == 3 {
                continue; // The bounded IWRAM continuation is tested separately.
            }
            let mut bus = Memory::new(vec![]).unwrap();
            let mut cpu = Cpu::new(pc);
            cpu.registers[0] = UNUSED;
            let expected = if thumb {
                cpu.instruction_set = InstructionSet::Thumb;
                bus.write16(pc, PROBE).unwrap();
                bus.write32(pc + 4, 0x83e6_92ab).unwrap();
                if pc >> 24 == 7 {
                    0x83e6_92ab
                } else {
                    0x92ab_92ab
                }
            } else {
                bus.write32(pc, 0xe590_6000).unwrap(); // LDR r6,[r0].
                bus.write32(pc + 8, 0x83e6_92ab).unwrap();
                0x83e6_92ab
            };
            bus.write32(EWRAM, DATA).unwrap();
            let mut machine = Machine::new(cpu, bus);
            transfer(&mut machine, EWRAM, IWRAM, true);
            machine.step().unwrap();
            assert_eq!(machine.cpu().registers()[6], expected);
        }
    }
}

#[test]
fn history_changes_add_no_timing_and_match_timed_and_untimed_cpu_results() {
    let mut machine = ready(CODE);
    machine
        .memory_mut()
        .write32(TIMER_BASE, 0x0080_0000)
        .unwrap();
    let start = machine.cycles();
    machine.memory_mut().write32(EWRAM, DATA).unwrap();
    transfer(&mut machine, EWRAM, IWRAM, true);
    assert_eq!(machine.last_timing().code_cycles, 0);
    assert_eq!(machine.last_timing().data_cycles, 7);
    assert_eq!(machine.last_timing().internal_cycles, 2);
    machine.step().unwrap();
    assert_eq!(machine.last_timing().code_cycles, 1);
    assert_eq!(machine.last_timing().data_cycles, 1);
    assert_eq!(machine.last_timing().internal_cycles, 1);
    assert_eq!(machine.cycles() - start, 12);
    assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), 12);

    let (mut cpu, mut bus) = program(CODE, 0x680a);
    cpu.step(&mut bus).unwrap();
    bus.write32(EWRAM, DATA).unwrap();
    configure(&mut bus, 3, EWRAM, IWRAM, 1, true);
    bus.step_dma().unwrap().unwrap();
    cpu.step(&mut bus).unwrap();
    assert_eq!(&cpu, machine.cpu());
    assert_eq!(bus.cycles(), 9); // Only explicit DMA stepping advances clocks here.
}
