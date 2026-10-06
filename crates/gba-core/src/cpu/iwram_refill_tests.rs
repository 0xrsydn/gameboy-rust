//! Original control-flow programs for Thumb IWRAM refill-lane snapshots.
use super::*;
use crate::{
    dma::{DMA_BASE, DMA_STRIDE},
    io::{IE, IME, TIMER_BASE},
    machine::{Machine, StepKind},
    memory::{BIOS_SIZE, ROM_START},
};

const SOURCE: u32 = 0x0300_0100;
const TARGET: u32 = 0x0300_0200;
const STACK: u32 = 0x0300_1000;
const UNUSED: u32 = 0x8000_0000;
const PROBE: u16 = 0x6806; // LDR r6,[r0]
const MARKER: u16 = 0x91ab;
const LOOKAHEAD: u16 = 0xd234;

fn target(bus: &mut Memory, pc: u32, instruction: u16) {
    for (index, half) in [instruction, MARKER, LOOKAHEAD, 0x5678]
        .into_iter()
        .enumerate()
    {
        bus.write16(pc + index as u32 * 2, half).unwrap();
    }
}

fn expected(pc: u32) -> u32 {
    if pc & 2 == 0 {
        0x91ab_d234
    } else {
        0xd234_91ab
    }
}

fn branch(source: u32, thumb: bool, destination: u32) -> (Cpu, Memory) {
    let instruction: u32 = if thumb { 0x46c0_4720 } else { 0xe12f_ff14 }; // BX r4
    let mut bus = if source < BIOS_SIZE as u32 {
        let mut bios = vec![0; BIOS_SIZE];
        bios[source as usize..source as usize + 4].copy_from_slice(&instruction.to_le_bytes());
        Memory::with_bios(vec![], bios).unwrap()
    } else if source == ROM_START {
        Memory::new(instruction.to_le_bytes().to_vec()).unwrap()
    } else {
        let mut bus = Memory::new(vec![]).unwrap();
        bus.write32(source, instruction).unwrap();
        bus
    };
    target(&mut bus, destination, PROBE);
    let mut cpu = Cpu::new(source);
    cpu.instruction_set = if thumb {
        InstructionSet::Thumb
    } else {
        InstructionSet::Arm
    };
    cpu.registers[0] = UNUSED;
    cpu.registers[4] = destination | 1;
    (cpu, bus)
}

#[test]
fn arm_and_thumb_bx_establish_history_at_both_target_alignments_and_mirrors() {
    for source in [0x100, 0x0200_0100, SOURCE, ROM_START] {
        for thumb in [false, true] {
            for destination in [
                TARGET,
                TARGET + 2,
                TARGET + 0x8000,
                0x0300_7ffc,
                0x0300_7ffe,
            ] {
                let (mut cpu, mut bus) = branch(source, thumb, destination);
                let status = cpu.cpsr() | 0xb000_00c0;
                cpu.apply_status(status, Mode::System);
                cpu.step(&mut bus).unwrap();
                assert_eq!(cpu.pc(), destination);
                assert_eq!(cpu.instruction_set(), InstructionSet::Thumb);
                cpu.step(&mut bus).unwrap();
                assert_eq!(cpu.registers[6], expected(destination));
                assert_eq!(cpu.cpsr(), status | 0x20);
                assert_eq!(cpu.registers[4], destination | 1);
            }
        }
    }
}

#[test]
fn refill_samples_target_plus_two_at_branch_time_but_plus_four_on_arrival() {
    for destination in [TARGET, TARGET + 2] {
        let (mut cpu, mut bus) = branch(ROM_START, false, destination);
        cpu.step(&mut bus).unwrap();
        bus.write16(destination + 2, 0xeeee).unwrap();
        bus.write16(destination + 4, 0xabcd).unwrap();
        assert_eq!(bus.read16(destination + 2).unwrap(), 0xeeee);
        cpu.step(&mut bus).unwrap();
        assert_eq!(
            cpu.registers[6],
            if destination & 2 == 0 {
                0x91ab_abcd
            } else {
                0xabcd_91ab
            }
        );
    }
}

#[test]
fn refill_history_supports_normal_load_lanes_rotation_and_sign_extension() {
    for destination in [TARGET, TARGET + 2] {
        let word = expected(destination);
        for (instruction, width, signed) in [
            (PROBE, 4, false),
            (0x7806, 1, false),
            (0x8806, 2, false),
            (0x5746, 1, true),
            (0x5f46, 2, true), // r5=0 offset, r6 result.
        ] {
            for lane in 0..4 {
                let (mut cpu, mut bus) = branch(ROM_START, false, destination);
                target(&mut bus, destination, instruction);
                cpu.registers[0] += lane;
                cpu.step(&mut bus).unwrap();
                let byte = (word >> (lane * 8)) as u8;
                let half = (word >> ((lane & 2) * 8)) as u16;
                let value = match (width, signed) {
                    (4, _) => word.rotate_right(lane * 8),
                    (1, false) => u32::from(byte),
                    (1, true) => byte as i8 as i32 as u32,
                    (2, false) => u32::from(half).rotate_right((lane & 1) * 8),
                    (2, true) if lane & 1 != 0 => byte as i8 as i32 as u32,
                    (2, true) => half as i16 as i32 as u32,
                    _ => unreachable!(),
                };
                let mut after = cpu.clone();
                after.registers[6] = value;
                after.registers[15] += 2;
                cpu.step(&mut bus).unwrap();
                assert_cpu_arch_eq!(cpu, after);
            }
        }
    }
}

#[test]
fn thumb_pc_writes_refill_after_data_loads_and_preserve_transfer_semantics() {
    for destination in [TARGET, TARGET + 2] {
        for instruction in [0x4720, 0x46a7, 0x44a7, 0xbd00, 0xbc00, 0xcc00] {
            // BX r4, MOV pc,r4, ADD pc,r4, POP {pc}, empty POP, empty LDM r4!.
            let (mut cpu, mut bus) = branch(SOURCE, true, destination);
            bus.write16(SOURCE, instruction).unwrap();
            bus.write32(STACK, destination).unwrap();
            cpu.registers[13] = STACK;
            cpu.registers[4] = match instruction {
                0xcc00 => STACK,
                0x44a7 => destination.wrapping_sub(SOURCE + 4),
                _ => destination | 1,
            };
            cpu.step(&mut bus).unwrap();
            assert_eq!(cpu.pc(), destination);
            assert_eq!(cpu.instruction_set(), InstructionSet::Thumb);
            if instruction == 0xbd00 {
                assert_eq!(cpu.registers[13], STACK + 4);
            }
            if instruction == 0xbc00 {
                assert_eq!(cpu.registers[13], STACK + 64);
            }
            if instruction == 0xcc00 {
                assert_eq!(cpu.registers[4], STACK + 64);
            }
            cpu.step(&mut bus).unwrap();
            assert_eq!(
                cpu.registers[6],
                expected(destination),
                "instruction={instruction:#x}"
            );
        }
    }
}

#[test]
fn thumb_taken_branches_and_bl_suffix_establish_target_history() {
    for destination in [TARGET, TARGET + 2] {
        for kind in 0..3 {
            let (mut cpu, mut bus) = branch(SOURCE, true, destination);
            if kind == 2 {
                bus.write16(SOURCE, 0xf000).unwrap(); // BL prefix: high offset zero.
                bus.write16(SOURCE + 2, 0xf800 | ((destination - SOURCE - 4) / 2) as u16)
                    .unwrap();
                cpu.step(&mut bus).unwrap();
                assert_eq!(cpu.pc(), SOURCE + 2);
            } else {
                let offset = ((destination - SOURCE - 4) / 2) as u16;
                bus.write16(
                    SOURCE,
                    if kind == 0 {
                        0xe000 | offset
                    } else {
                        0xd000 | offset
                    },
                )
                .unwrap();
                cpu.flags.zero = true;
            }
            cpu.step(&mut bus).unwrap();
            assert_eq!(cpu.pc(), destination);
            if kind == 2 {
                assert_eq!(cpu.registers[14], (SOURCE + 4) | 1);
            }
            cpu.step(&mut bus).unwrap();
            assert_eq!(cpu.registers[6], expected(destination));
        }
    }
}

#[test]
fn arm_exception_returns_use_saved_thumb_state_and_restored_banks() {
    for destination in [TARGET, TARGET + 2] {
        for mode in [Mode::User, Mode::System] {
            for instruction in [0xe1b0_f00e_u32, 0xe25e_f004, 0xe8fd_8004] {
                // MOVS pc,lr; SUBS pc,lr,#4; LDMIA sp!,{r2,pc}^.
                let mut bus = Memory::new(instruction.to_le_bytes().to_vec()).unwrap();
                target(&mut bus, destination, PROBE);
                bus.write32(STACK, 0xfeed_beef).unwrap();
                bus.write32(STACK + 4, destination).unwrap();
                let mut cpu = Cpu::new(ROM_START);
                cpu.registers[0] = UNUSED;
                cpu.registers[13] = 0x0300_7000;
                cpu.registers[14] = 0x1234;
                cpu.enter_exception(Exception::SoftwareInterrupt);
                cpu.registers[15] = ROM_START;
                cpu.registers[13] = STACK;
                cpu.registers[14] = destination + if instruction == 0xe25e_f004 { 4 } else { 0 };
                let status = 0xa000_00e0 | mode as u32;
                cpu.set_spsr(status);
                cpu.step(&mut bus).unwrap();
                assert_eq!(cpu.pc(), destination);
                assert_eq!(cpu.cpsr(), status);
                assert_eq!(cpu.registers[13], 0x0300_7000);
                assert_eq!(cpu.registers[14], 0x1234);
                if instruction == 0xe8fd_8004 {
                    assert_eq!(cpu.registers[2], 0xfeed_beef);
                }
                cpu.step(&mut bus).unwrap();
                assert_eq!(cpu.registers[6], expected(destination));
                cpu.switch_mode(Mode::Supervisor);
                assert_eq!(
                    cpu.registers[13],
                    STACK + if instruction == 0xe8fd_8004 { 8 } else { 0 }
                );
            }
        }
    }
}

#[test]
fn real_swi_and_irq_returns_can_resume_thumb_iwram_with_known_lanes() {
    for irq in [false, true] {
        let destination = if irq { TARGET } else { TARGET + 2 };
        let mut bios = vec![0; BIOS_SIZE];
        bios[8..12].copy_from_slice(&0xe1b0_f00e_u32.to_le_bytes());
        bios[0x18..0x1c].copy_from_slice(&0xe25e_f004_u32.to_le_bytes());
        let mut bus = Memory::with_bios(vec![], bios).unwrap();
        target(&mut bus, destination, PROBE);
        if !irq {
            bus.write16(TARGET, 0xdf00).unwrap();
        }
        let mut cpu = Cpu::new(TARGET);
        cpu.instruction_set = InstructionSet::Thumb;
        cpu.registers[0] = UNUSED;
        let status = cpu.cpsr();
        let mut machine = Machine::new(cpu, bus);
        if irq {
            machine.memory_mut().write16(IE, 8).unwrap();
            machine.memory_mut().write16(IME, 1).unwrap();
            machine
                .memory_mut()
                .write32(TIMER_BASE, 0x00c0_ffff)
                .unwrap();
            machine.memory_mut().advance_cycles(1);
            assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
            machine.memory_mut().write16(IME, 0).unwrap(); // Keep this test to one IRQ entry.
        } else {
            machine.step().unwrap();
        }
        machine.step().unwrap(); // Execute the original ARM return handler.
        assert_eq!(machine.cpu().pc(), destination);
        assert_eq!(machine.cpu().cpsr(), status);
        machine.step().unwrap();
        assert_eq!(machine.cpu().registers()[6], expected(destination));
    }
}

#[test]
fn failed_refill_instructions_preserve_cpu_and_preexisting_history() {
    for instruction in [0xe1b0_f00e_u32, 0xe8fd_8004] {
        let mut bus = Memory::new(instruction.to_le_bytes().to_vec()).unwrap();
        for (index, half) in [0x680a_u16, PROBE, 0x1111, LOOKAHEAD]
            .into_iter()
            .enumerate()
        {
            bus.write16(SOURCE + index as u32 * 2, half).unwrap();
        }
        bus.write32(STACK, 0x1234_abcd).unwrap();
        let mut paused = Cpu::new(SOURCE);
        paused.instruction_set = InstructionSet::Thumb;
        paused.registers[0] = UNUSED;
        paused.registers[1] = STACK;
        paused.step(&mut bus).unwrap(); // Seed sequential history with a word load.
        let mut failing = Cpu::at_reset();
        failing.registers[15] = ROM_START;
        failing.registers[13] = STACK;
        failing.registers[14] = TARGET;
        failing.set_spsr(0); // Invalid return mode; no refill must commit.
        let before = failing.clone();
        assert!(failing.step_timed(&mut bus).is_err());
        assert_eq!(failing, before);
        paused.step(&mut bus).unwrap();
        assert_eq!(paused.registers[6], 0xd234_abcd);
    }
}

#[test]
fn cold_fill_establishes_lanes_but_io_crossings_and_unmapped_targets_stay_diagnostic() {
    let mut bus = Memory::new(vec![]).unwrap();
    target(&mut bus, TARGET, PROBE);
    let mut cpu = Cpu::new(TARGET);
    cpu.instruction_set = InstructionSet::Thumb;
    cpu.registers[0] = UNUSED;
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[6], expected(TARGET));

    for destination in [0x03ff_fffc, 0x03ff_fffe, 0x0e00_0000] {
        let mut bus = Memory::new(0xe12f_ff14_u32.to_le_bytes().to_vec()).unwrap();
        if destination >> 24 == 3 {
            bus.write16(destination, PROBE).unwrap();
        }
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[0] = UNUSED;
        cpu.registers[4] = destination | 1;
        cpu.step(&mut bus).unwrap(); // An unavailable target/refill never fails BX early.
        assert_eq!(cpu.pc(), destination);
        let before = cpu.clone();
        assert_eq!(
            cpu.step(&mut bus),
            Err(CpuError::Memory(MemoryError::Unmapped(
                if destination >> 24 == 3 {
                    UNUSED
                } else {
                    destination
                }
            )))
        );
        assert_eq!(cpu, before);
    }
}

#[test]
fn target_pair_refill_does_not_replace_the_separate_bios_retained_word() {
    for destination in [TARGET, TARGET + 2] {
        let mut bios = vec![0; BIOS_SIZE];
        bios[0x100..0x104].copy_from_slice(&0xe12f_ff14_u32.to_le_bytes());
        bios[0x108..0x10c].copy_from_slice(&0x89ab_cdef_u32.to_le_bytes());
        let mut bus = Memory::with_bios(vec![], bios).unwrap();
        target(&mut bus, destination, PROBE);
        let mut cpu = Cpu::new(0x100);
        cpu.registers[4] = destination | 1;
        cpu.step(&mut bus).unwrap();
        cpu.step(&mut bus).unwrap(); // r0=0: protected BIOS read, not unused memory.
        assert_eq!(cpu.registers[6], 0x89ab_cdef);
        assert_eq!(bus.read32(0x108).unwrap(), 0x89ab_cdef);
    }
}

#[test]
fn successful_dma_between_refill_and_target_execution_updates_refill_history() {
    let (cpu, mut bus) = branch(ROM_START, false, TARGET);
    bus.write32(STACK, 0x1234_abcd).unwrap();
    let mut machine = Machine::new(cpu, bus);
    machine.step().unwrap();
    let dma = DMA_BASE + 3 * DMA_STRIDE;
    machine.memory_mut().write32(dma, STACK).unwrap();
    machine.memory_mut().write32(dma + 4, 0x0200_0000).unwrap();
    machine.memory_mut().write32(dma + 8, 0x8400_0001).unwrap();
    assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 3 });
    let cycles = machine.cycles();
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    // DMA reads the whole IWRAM word, then the target+4 fetch replaces its low half.
    assert_eq!(machine.cpu().registers()[6], 0x1234_d234);
    assert_eq!(machine.cycles(), cycles + 3);
    assert_eq!(machine.cpu().pc(), TARGET + 2);
}

#[test]
fn refill_snapshots_keep_existing_nominal_costs_and_timed_untimed_results() {
    for thumb in [false, true] {
        for destination in [TARGET, TARGET + 2] {
            let (cpu, mut bus) = branch(ROM_START, thumb, destination);
            bus.write32(TIMER_BASE, 0x0080_0000).unwrap();
            let mut machine = Machine::new(cpu, bus);
            let (mut untimed, mut bus) = branch(ROM_START, thumb, destination);
            for index in 0..2 {
                untimed.step(&mut bus).unwrap();
                machine.step().unwrap();
                assert_eq!(machine.cpu(), &untimed);
                assert_eq!(
                    machine.last_timing().code_cycles,
                    if index == 0 { 3 } else { 1 }
                );
                assert_eq!(machine.last_timing().data_cycles, index);
                assert_eq!(machine.last_timing().internal_cycles, index);
            }
            assert_eq!(machine.cycles(), 6);
            assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), 6);
            assert_eq!(bus.cycles(), 0);
        }
    }
}
