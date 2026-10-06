//! Thumb BIOS fetches drive a complete aligned word, unlike 16-bit ROM open bus.
use super::*;

const THUMB_ENTRY: u32 = 0x300;
const BX_R5: u16 = 0x4728;

fn half(bytes: &mut [u8], address: u32, value: u16) {
    bytes[address as usize..address as usize + 2].copy_from_slice(&value.to_le_bytes());
}

fn thumb_image(pc: u32, instruction: u16) -> Vec<u8> {
    let mut bios = image();
    half(&mut bios, pc, instruction);
    if pc < 0x3ffc {
        word(&mut bios, (pc + 4) & !3, OTHER);
    }
    bios
}

fn enter_thumb(memory: &mut Memory, pc: u32, target_thumb: bool) -> Cpu {
    let mut cpu = Cpu::new(ENTRY);
    cpu.registers[4] = pc | 1;
    cpu.registers[5] = ROM_START | u32::from(target_thumb);
    cpu.step(memory).unwrap(); // Real ARM BX into Thumb BIOS, seed ARM history.
    assert_eq!(cpu.pc(), pc);
    assert_eq!(cpu.instruction_set(), InstructionSet::Thumb);
    cpu
}

// Inspect via an independent CPU load without changing the BIOS CPU's state.
fn protected_word(memory: &mut Memory) -> u32 {
    memory.write32(0x0200_0000, LDR).unwrap();
    let mut probe = Cpu::new(0x0200_0000);
    probe.step(memory).unwrap();
    probe.registers[1]
}

#[test]
fn thumb_exit_retains_both_word_lanes_for_each_code_alignment_and_load_width() {
    for offset in [0, 2] {
        let pc = THUMB_ENTRY + offset;
        for (arm, thumb, expected) in [
            (
                LDR,
                0x6801,
                [0xf233_80a5, 0xa5f2_3380, 0x80a5_f233, 0x3380_a5f2],
            ),
            (0xe5d0_1000, 0x7801, [0xa5, 0x80, 0x33, 0xf2]),
            (
                0xe1d0_10b0,
                0x8801,
                [0x80a5, 0xa500_0080, 0xf233, 0x3300_00f2],
            ),
            (
                0xe1d0_10d0,
                0x5681,
                [0xffff_ffa5, 0xffff_ff80, 0x33, 0xffff_fff2],
            ),
            (
                0xe1d0_10f0,
                0x5e81,
                [0xffff_80a5, 0xffff_ff80, 0xffff_f233, 0xffff_fff2],
            ),
        ] {
            for target_thumb in [false, true] {
                for address in [0, 0x100, 0x3ffc] {
                    for (lane, value) in expected.into_iter().enumerate() {
                        let mut bios = thumb_image(pc, BX_R5);
                        word(&mut bios, ENTRY + 8, OTHER); // Distinct incoming ARM history.
                        word(&mut bios, THUMB_ENTRY + 4, DATA);
                        // Poison PC+8, and PC+6's next word for the unaligned case.
                        word(&mut bios, THUMB_ENTRY + 8, 0xdead_beef);
                        let mut memory = bus(if target_thumb { thumb } else { arm }, bios);
                        let mut cpu = enter_thumb(&mut memory, pc, target_thumb);
                        cpu.step(&mut memory).unwrap();
                        assert_eq!(cpu.pc(), ROM_START);
                        cpu.registers[0] = address + lane as u32;
                        cpu.flags.carry = true;
                        let mut after = cpu.clone();
                        after.registers[1] = value;
                        after.registers[15] += if target_thumb { 2 } else { 4 };
                        let timing = cpu.step_timed(&mut memory).unwrap();
                        assert_cpu_arch_eq!(
                            cpu,
                            after,
                            "pc={pc:#x} lane={lane} thumb={target_thumb}"
                        );
                        assert_eq!(timing.data_cycles, 1);
                        assert_eq!(timing.internal_cycles, 1);
                        assert_eq!(memory.cycles(), 0);
                    }
                }
            }
        }
    }
}

#[test]
fn sequential_thumb_steps_sample_the_aligned_fetch_word_without_extra_cycles() {
    let mut bios = thumb_image(THUMB_ENTRY, 0x46c0); // MOV r8,r8
    half(&mut bios, THUMB_ENTRY + 2, 0x46c0);
    half(&mut bios, THUMB_ENTRY + 4, BX_R5);
    // At PC+4 and PC+6: the exit instruction plus a distinct high halfword.
    let first = (OTHER & 0xffff_0000) | u32::from(BX_R5);
    word(&mut bios, THUMB_ENTRY + 8, DATA);
    let mut memory = bus(LDR, bios);
    let mut cpu = enter_thumb(&mut memory, THUMB_ENTRY, false);
    assert_eq!(protected_word(&mut memory), DATA); // Incoming ARM prefetch.
    for expected in [first, first, DATA] {
        let timing = cpu.step_timed(&mut memory).unwrap();
        assert_eq!(timing.data_cycles, 0);
        assert_eq!(protected_word(&mut memory), expected);
        assert_eq!(memory.cycles(), 0);
    }
    assert_eq!(cpu.pc(), ROM_START);
}

#[test]
fn thumb_to_arm_bios_transition_uses_incoming_state_then_refreshes_arm_history() {
    for offset in [0, 2] {
        let mut bios = thumb_image(THUMB_ENTRY + offset, BX_R5);
        word(&mut bios, 0x208, DATA);
        let mut memory = bus(LDR, bios);
        let mut cpu = enter_thumb(&mut memory, THUMB_ENTRY + offset, false);
        cpu.registers[5] = 0x200; // Existing ARM BX r4, now with DATA at PC+8.
        cpu.registers[4] = ROM_START;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.instruction_set(), InstructionSet::Arm);
        assert_eq!(protected_word(&mut memory), OTHER);
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.pc(), ROM_START);
        assert_eq!(protected_word(&mut memory), DATA);
    }
}

#[test]
fn thumb_bios_protection_remains_pc_based_in_every_processor_mode() {
    for mode in [
        Mode::User,
        Mode::System,
        Mode::Fiq,
        Mode::Irq,
        Mode::Supervisor,
        Mode::Abort,
        Mode::Undefined,
    ] {
        for offset in [0, 2] {
            let pc = THUMB_ENTRY + offset;
            let mut bios = thumb_image(pc, 0x6801); // LDR r1,[r0] inside BIOS.
            half(&mut bios, pc + 2, BX_R5);
            let mut memory = bus(LDR, bios);
            let mut cpu = enter_thumb(&mut memory, pc, false);
            cpu.apply_status(0xb000_00e0 | mode as u32, mode);
            cpu.registers[0] = ENTRY + 8;
            cpu.step(&mut memory).unwrap();
            assert_eq!(cpu.registers[1], DATA); // Raw BIOS data, not retained OTHER.
            assert_eq!(cpu.cpsr(), 0xb000_00e0 | mode as u32);
            cpu.step(&mut memory).unwrap(); // BX to ARM ROM.
            let expected = memory.read32((pc + 6) & !3).unwrap();
            cpu.registers[0] = ENTRY + 8;
            cpu.step(&mut memory).unwrap();
            assert_eq!(cpu.registers[1], expected);
            assert_eq!(cpu.cpsr(), 0xb000_00c0 | mode as u32);
        }
    }
}

#[test]
fn thumb_pop_pc_retains_prefetch_not_stack_data_and_stays_thumb() {
    for offset in [0, 2] {
        let mut memory = bus(0x6801, thumb_image(THUMB_ENTRY + offset, 0xbd00)); // POP {pc}
        let mut cpu = enter_thumb(&mut memory, THUMB_ENTRY + offset, true);
        cpu.registers[13] = 0x0300_0100;
        memory.write32(0x0300_0100, ROM_START).unwrap();
        let timing = cpu.step_timed(&mut memory).unwrap();
        assert_eq!(timing.data_cycles, 1);
        assert_eq!(cpu.registers[13], 0x0300_0104);
        assert_eq!(cpu.instruction_set(), InstructionSet::Thumb);
        assert_eq!(cpu.pc(), ROM_START);
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[1], OTHER);
    }
}

#[test]
fn thumb_swi_enters_arm_and_the_next_bios_instruction_replaces_thumb_history() {
    for offset in [0, 2] {
        let mut bios = thumb_image(THUMB_ENTRY + offset, 0xdf00); // SWI #0
        word(&mut bios, 8, 0xe12f_ff15); // BX r5
        word(&mut bios, 16, DATA);
        let mut memory = bus(LDR, bios);
        let mut cpu = enter_thumb(&mut memory, THUMB_ENTRY + offset, false);
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.pc(), 8);
        assert_eq!(cpu.mode(), Mode::Supervisor);
        assert_eq!(cpu.instruction_set(), InstructionSet::Arm);
        assert_eq!(protected_word(&mut memory), OTHER);
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.pc(), ROM_START);
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[1], DATA);
    }
}

#[test]
fn last_complete_thumb_fetch_word_is_valid_at_both_alignments() {
    for pc in [0x3ff8, 0x3ffa] {
        let mut memory = bus(LDR, thumb_image(pc, BX_R5));
        let mut cpu = enter_thumb(&mut memory, pc, false);
        cpu.step(&mut memory).unwrap();
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[1], OTHER);
    }
    for pc in [0x3ffc, 0x3ffe] {
        let mut memory = bus(LDR, thumb_image(pc, BX_R5));
        let mut cpu = enter_thumb(&mut memory, pc, false);
        cpu.step(&mut memory).unwrap(); // Missing lookahead does not fail BX itself.
        assert_eq!(cpu.pc(), ROM_START);
        let before = cpu.clone();
        assert_eq!(
            cpu.step(&mut memory),
            Err(CpuError::Memory(MemoryError::Unmapped(0)))
        );
        assert_eq!(cpu, before);
    }
}

#[test]
fn failed_thumb_steps_keep_previous_history_and_clear_access_context() {
    for instruction in [0xde00, 0x6801] {
        // Undefined instruction / failing LDR.
        let mut memory = bus(LDR, thumb_image(THUMB_ENTRY, instruction));
        let mut cpu = enter_thumb(&mut memory, THUMB_ENTRY, false);
        cpu.registers[0] = 0x0e00_0000;
        let before = cpu.clone();
        assert!(cpu.step_timed(&mut memory).is_err());
        assert_eq!(cpu, before);
        assert_eq!(memory.cycles(), 0);
        assert_eq!(memory.read32(THUMB_ENTRY + 4).unwrap(), OTHER); // Raw host read.
        assert_eq!(protected_word(&mut memory), DATA); // Previous ARM history retained.
    }
}

#[test]
fn outside_thumb_execution_keeps_bios_history_separate_from_cold_iwram_fills() {
    let mut memory = bus(0x6801, thumb_image(THUMB_ENTRY, BX_R5));
    let mut cpu = enter_thumb(&mut memory, THUMB_ENTRY, true);
    cpu.step(&mut memory).unwrap();
    assert_eq!(protected_word(&mut memory), OTHER);
    memory.write16(0x0300_0100, 0x6801).unwrap(); // Cold IWRAM entry.
    memory.write16(0x0300_0102, 0x6801).unwrap(); // Next protected BIOS read.
    memory.write16(0x0300_0104, 0x1357).unwrap();
    cpu.registers[15] = 0x0300_0100;
    cpu.registers[0] = 0x4000;
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[1], 0x6801_1357);
    assert_eq!(memory.read32(0x4000), Err(MemoryError::Unmapped(0x4000)));
    assert_eq!(protected_word(&mut memory), OTHER);
    cpu.registers[0] = 0;
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[1], OTHER);
}

#[test]
fn machine_steps_charge_thumb_exit_and_protected_load_without_snapshot_accesses() {
    for offset in [0, 2] {
        let mut memory = bus(LDR, thumb_image(THUMB_ENTRY + offset, BX_R5));
        let cpu = enter_thumb(&mut memory, THUMB_ENTRY + offset, false);
        memory.write32(TIMER_BASE, 0x0080_0000).unwrap();
        let mut machine = Machine::new(cpu, memory);
        assert_eq!(machine.step().unwrap(), StepKind::Instruction);
        let cycles = machine.cycles();
        assert_eq!(machine.last_timing().data_cycles, 0);
        assert_eq!(machine.step().unwrap(), StepKind::Instruction);
        assert_eq!(machine.cpu().registers()[1], OTHER);
        assert_eq!(
            machine.last_timing(),
            StepTiming {
                code_cycles: 6, // Returned ARM code fetches at ROM_START+8.
                data_cycles: 1,
                internal_cycles: 1,
                idle_cycles: 0,
            }
        );
        assert_eq!(machine.cycles(), cycles + 8);
        assert_eq!(
            u64::from(machine.memory().read16(TIMER_BASE).unwrap()),
            machine.cycles()
        );
    }
}
