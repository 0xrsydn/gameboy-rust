use super::*;
use crate::{io::WAITCNT, memory::ROM_START, timing::StepTiming};

const CODE: u32 = 0x0300_0000;
const DATA: u32 = 0x0200_0100;

fn arm(instruction: u32) -> (Cpu, Memory) {
    let mut memory = Memory::new(vec![0; 0x40000]).unwrap();
    memory.write32(CODE, instruction).unwrap();
    (Cpu::new(CODE), memory)
}

fn thumb(instruction: u16) -> (Cpu, Memory) {
    let (mut cpu, mut memory) = arm(0);
    cpu.instruction_set = InstructionSet::Thumb;
    memory.write16(CODE, instruction).unwrap();
    (cpu, memory)
}

fn expected(code: u32, data: u32, internal: u32) -> StepTiming {
    StepTiming {
        code_cycles: code,
        data_cycles: data,
        internal_cycles: internal,
        idle_cycles: 0,
    }
}

#[test]
fn arm_alu_immediate_shifts_register_shifts_and_status_transfers() {
    for opcode in 0..16 {
        for operand in [0x0200_0001, 0x0000_0082, 0x0000_0312] {
            // r0 destination, r1 first operand; S set makes TST/TEQ/CMP/CMN valid.
            let instruction = 0xe011_0000 | (opcode << 21) | operand;
            let (mut cpu, mut memory) = arm(instruction);
            cpu.registers[1] = 123;
            cpu.registers[2] = 12;
            cpu.registers[3] = 0; // Register shift still costs I when amount is zero.
            let cycles = cpu.step_timed(&mut memory).unwrap();
            assert_eq!(cycles, expected(1, 0, u32::from(operand == 0x312)));
            assert_eq!(memory.cycles(), 0); // Timed CPU API returns costs; it does not run devices.
        }
    }
    for instruction in [0xe10f_0000, 0xe328_f480, 0xe128_f000, 0xe321_f013] {
        let (mut cpu, mut memory) = arm(instruction);
        assert_eq!(cpu.step_timed(&mut memory).unwrap(), expected(1, 0, 0));
    }
}

#[test]
fn conditional_skips_do_not_charge_memory_multiply_or_branch_work() {
    for instruction in [
        0x0590_1000,
        0x0580_1000,
        0x0a00_0000,
        0x0f00_0000,
        0x0000_0291,
    ] {
        let (mut cpu, mut memory) = arm(instruction);
        cpu.registers[2] = 0x1234_5678;
        assert_eq!(cpu.step_timed(&mut memory).unwrap(), expected(1, 0, 0));
        assert_eq!(cpu.pc(), CODE + 4);
    }
}

#[test]
fn branch_and_pc_write_refill_even_when_target_equals_fallthrough() {
    for instruction in [0xeaff_ffff, 0xe1a0_f000, 0xe1a0_f110] {
        let (mut cpu, mut memory) = arm(instruction);
        cpu.registers[0] = CODE + 4;
        let timing = cpu.step_timed(&mut memory).unwrap();
        assert_eq!(
            timing,
            expected(3, 0, u32::from(instruction == 0xe1a0_f110))
        );
        assert_eq!(cpu.pc(), CODE + 4);
    }
}

#[test]
fn cross_region_bx_uses_source_and_destination_widths_and_wait_states() {
    let (mut cpu, mut memory) = arm(0xe12f_ff10);
    cpu.registers[0] = 0x0a00_0101; // IWRAM source 1 + Thumb WS1 target pair 5N + 5S.
    assert_eq!(cpu.step_timed(&mut memory).unwrap(), expected(11, 0, 0));
    assert_eq!(cpu.instruction_set(), InstructionSet::Thumb);
    let (mut cpu, mut memory) = thumb(0x4700);
    cpu.registers[0] = ROM_START + 0x100; // IWRAM source 1 + ARM WS0 target pair 8N + 6S.
    assert_eq!(cpu.step_timed(&mut memory).unwrap(), expected(15, 0, 0));
}

#[test]
fn arm_single_load_store_widths_charge_one_actual_bus_access() {
    for (instruction, data_cycles, internal) in [
        (0xe590_1000, 6, 1), // LDR
        (0xe580_1000, 6, 0), // STR
        (0xe5d0_1000, 3, 1), // LDRB
        (0xe5c0_1000, 3, 0), // STRB
        (0xe1d0_10b0, 3, 1), // LDRH
        (0xe1c0_10b0, 3, 0), // STRH
        (0xe1d0_10d0, 3, 1), // LDRSB
        (0xe1d0_10f0, 3, 1), // LDRSH
    ] {
        for offset in 0..4 {
            let (mut cpu, mut memory) = arm(instruction);
            cpu.registers[0] = DATA + offset;
            assert_eq!(
                cpu.step_timed(&mut memory).unwrap(),
                expected(1, data_cycles, internal)
            );
        }
    }
}

#[test]
fn actual_pre_post_and_shifted_addresses_determine_load_costs() {
    for (instruction, base, offset) in [
        (0xe790_1102, 0x0100_0100, 0x0080_0000), // LDR r1,[r0,r2,LSL #2] -> IWRAM
        (0xe490_1004, 0x0300_0100, 0),           // post-index uses old base
        (0xe510_1100, 0x0300_0200, 0),           // negative immediate -> IWRAM
    ] {
        let (mut cpu, mut memory) = arm(instruction);
        cpu.registers[0] = base;
        cpu.registers[2] = offset;
        assert_eq!(cpu.step_timed(&mut memory).unwrap(), expected(1, 1, 1));
    }
}

#[test]
fn load_pc_adds_refill_and_does_not_count_fetch_as_data() {
    let (mut cpu, mut memory) = arm(0xe590_f000);
    cpu.registers[0] = DATA;
    memory.write32(DATA, ROM_START + 0x100).unwrap();
    assert_eq!(cpu.step_timed(&mut memory).unwrap(), expected(15, 6, 1));
}

#[test]
fn block_transfers_count_words_and_apply_refill_only_for_load_pc() {
    for list in [0, 1, 0x000f, 0x1fff, 0xffff] {
        for load in [false, true] {
            let (mut cpu, mut memory) = arm(0xe880_0000 | (u32::from(load) << 20) | list);
            cpu.registers[0] = CODE + 0x100;
            let count = if list == 0 { 1 } else { list.count_ones() };
            let pc = load && (list == 0 || list & 0x8000 != 0);
            assert_eq!(
                cpu.step_timed(&mut memory).unwrap(),
                expected(if pc { 3 } else { 1 }, count, u32::from(load))
            );
        }
    }
}

#[test]
fn rom_block_reads_use_n_then_s_and_restart_at_128_kib_boundary() {
    let (mut cpu, mut memory) = arm(0xe890_001e); // LDMIA r0, {r1-r4}
    cpu.registers[0] = ROM_START + 0x1fff8;
    // Word accesses: N+S=8, S+S=6, forced N+S=8, S+S=6.
    assert_eq!(cpu.step_timed(&mut memory).unwrap(), expected(1, 28, 1));
    let (mut cpu, mut memory) = arm(0xe590_1000);
    cpu.registers[0] = ROM_START + 0x100;
    assert_eq!(cpu.step_timed(&mut memory).unwrap(), expected(1, 8, 1));
}

#[test]
fn swap_uses_two_nonsequential_accesses_and_one_internal_cycle() {
    for (instruction, data_cycles) in [(0xe100_1092, 12), (0xe140_1092, 6)] {
        let (mut cpu, mut memory) = arm(instruction);
        cpu.registers[0] = DATA;
        cpu.registers[2] = 123;
        assert_eq!(
            cpu.step_timed(&mut memory).unwrap(),
            expected(1, data_cycles, 1)
        );
    }
}

#[test]
fn multiply_early_termination_uses_rs_and_signedness_before_execution() {
    for (value, signed_m, unsigned_m) in [
        (0, 1, 1),
        (255, 1, 1),
        (256, 2, 2),
        (65535, 2, 2),
        (65536, 3, 3),
        (0x00ff_ffff, 3, 3),
        (0x0100_0000, 4, 4),
        (0xffff_ffff, 1, 4),
        (0xffff_ff00, 1, 4),
        (0xffff_0000, 2, 4),
        (0xff00_0000, 3, 4),
        (0x8000_0000, 4, 4),
    ] {
        for (instruction, internal) in [
            (0xe000_0291, signed_m),       // MUL r0,r1,r2
            (0xe020_3291, signed_m + 1),   // MLA r0,r1,r2,r3
            (0xe081_0392, unsigned_m + 1), // UMULL r0,r1,r2,r3
            (0xe0a1_0392, unsigned_m + 2), // UMLAL
            (0xe0c1_0392, signed_m + 1),   // SMULL
            (0xe0e1_0392, signed_m + 2),   // SMLAL
        ] {
            let (mut cpu, mut memory) = arm(instruction);
            cpu.registers[1] = 0x1234_5678;
            cpu.registers[2] = value;
            cpu.registers[3] = value;
            assert_eq!(
                cpu.step_timed(&mut memory).unwrap(),
                expected(1, 0, internal),
                "{instruction:08x}, {value:08x}"
            );
        }
    }
}

#[test]
fn thumb_alu_shifts_and_multiply_use_correct_internal_cycles() {
    for opcode in 0..16 {
        for value in [7, 0x1234_5678, 0xffff_ffff] {
            let (mut cpu, mut memory) = thumb(0x4008 | opcode << 6);
            cpu.registers[0] = value;
            cpu.registers[1] = 0; // Multiplication timing must not use this source.
            let internal = match opcode {
                2 | 3 | 4 | 7 => 1,
                13 if value == 0x1234_5678 => 4,
                13 => 1,
                _ => 0,
            };
            assert_eq!(
                cpu.step_timed(&mut memory).unwrap(),
                expected(1, 0, internal)
            );
        }
    }
}

#[test]
fn thumb_transfers_literals_sp_and_register_offsets_use_actual_bus_widths() {
    for (instruction, data_cycles, internal) in [
        (0x4800, 1, 1), // literal LDR from IWRAM
        (0x5081, 6, 0),
        (0x5281, 3, 0),
        (0x5481, 3, 0),
        (0x5681, 3, 1),
        (0x5881, 6, 1),
        (0x5a81, 3, 1),
        (0x5c81, 3, 1),
        (0x5e81, 3, 1),
        (0x6001, 6, 0),
        (0x6801, 6, 1),
        (0x7001, 3, 0),
        (0x7801, 3, 1),
        (0x8001, 3, 0),
        (0x8801, 3, 1),
        (0x9000, 6, 0),
        (0x9800, 6, 1),
    ] {
        let (mut cpu, mut memory) = thumb(instruction);
        cpu.registers[0] = DATA;
        cpu.registers[13] = DATA;
        assert_eq!(
            cpu.step_timed(&mut memory).unwrap(),
            expected(1, data_cycles, internal),
            "{instruction:04x}"
        );
    }
}

#[test]
fn thumb_branches_bl_halves_and_high_pc_writes_refill_only_when_taken() {
    for (instruction, code) in [
        (0xd000, 1),
        (0xd100, 3),
        (0xe000, 3),
        (0x4687, 3),
        (0x4487, 3),
        (0x4587, 1),
    ] {
        let (mut cpu, mut memory) = thumb(instruction);
        cpu.registers[0] = CODE + 4;
        assert_eq!(cpu.step_timed(&mut memory).unwrap(), expected(code, 0, 0));
    }
    let (mut cpu, mut memory) = thumb(0xf000);
    memory.write16(CODE + 2, 0xf800).unwrap();
    assert_eq!(cpu.step_timed(&mut memory).unwrap(), expected(1, 0, 0));
    assert_eq!(cpu.step_timed(&mut memory).unwrap(), expected(3, 0, 0));
}

#[test]
fn thumb_push_pop_and_multiple_transfers_charge_each_word() {
    for (instruction, data, internal, code) in [
        (0xb403, 12, 0, 1),
        (0xb503, 18, 0, 1),
        (0xbc03, 12, 1, 1),
        (0xbd03, 18, 1, 3),
        (0xc003, 12, 0, 1),
        (0xc803, 12, 1, 1),
        (0xc800, 6, 1, 3),
        (0xbc00, 6, 1, 3),
    ] {
        let (mut cpu, mut memory) = thumb(instruction);
        cpu.registers[0] = DATA;
        cpu.registers[13] = DATA;
        assert_eq!(
            cpu.step_timed(&mut memory).unwrap(),
            expected(code, data, internal)
        );
    }
}

#[test]
fn software_interrupts_and_exception_returns_use_new_code_region_and_state() {
    for thumb_state in [false, true] {
        let (mut cpu, mut memory) = if thumb_state {
            thumb(0xdf00)
        } else {
            arm(0xef00_0000)
        };
        assert_eq!(cpu.step_timed(&mut memory).unwrap(), expected(3, 0, 0));
        assert_eq!(cpu.mode(), Mode::Supervisor);
        // Return via MOVS pc,lr, executing from IWRAM without a BIOS image.
        memory.write32(CODE + 0x100, 0xe1b0_f00e).unwrap();
        cpu.registers[15] = CODE + 0x100;
        cpu.registers[14] = ROM_START + 0x100;
        assert_eq!(
            cpu.step_timed(&mut memory).unwrap(),
            expected(if thumb_state { 9 } else { 15 }, 0, 0)
        );
    }
}

#[test]
fn waitcnt_store_charges_old_code_settings_and_new_settings_apply_next_step() {
    let mut memory = Memory::new(
        [0xe580_1000_u32, 0xe1a0_0000]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect(),
    )
    .unwrap();
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = WAITCNT;
    cpu.registers[1] = 0x18; // N=3 and S=2 per halfword
    assert_eq!(cpu.step_timed(&mut memory).unwrap(), expected(8, 1, 0));
    assert_eq!(memory.waitcnt(), 0x18);
    assert_eq!(cpu.step_timed(&mut memory).unwrap(), expected(4, 0, 0));
}

#[test]
fn failed_data_access_discards_trace_before_retry() {
    let (mut cpu, mut memory) = arm(0xe8b0_0006); // LDMIA r0!,{r1,r2}
    cpu.registers[0] = 0x0400_0054;
    let before = cpu.clone();
    assert!(cpu.step_timed(&mut memory).is_err());
    assert_eq!(cpu, before);
    assert_eq!(memory.cycles(), 0);
    cpu.registers[0] = DATA;
    assert_eq!(cpu.step_timed(&mut memory).unwrap(), expected(1, 12, 1));
}

#[test]
fn timing_preserves_semantics_for_every_thumb_encoding() {
    let mut timed_memory = Memory::new(vec![]).unwrap();
    let mut plain_memory = Memory::new(vec![]).unwrap();
    for instruction in 0..=u16::MAX {
        timed_memory.write16(CODE, instruction).unwrap();
        plain_memory.write16(CODE, instruction).unwrap();
        let mut plain = Cpu::new(CODE);
        plain.instruction_set = InstructionSet::Thumb;
        for register in 0..15 {
            plain.registers[register] = DATA + register as u32 * 4;
        }
        let mut timed = plain.clone();
        let cost = timed.step_timed(&mut timed_memory);
        if let Ok(cost) = cost {
            assert!(cost.total() > 0);
        }
        assert_eq!(
            cost.map(|_| ()),
            plain.step(&mut plain_memory),
            "{instruction:04x}"
        );
        assert_eq!(timed, plain, "{instruction:04x}");
        assert_eq!(timed_memory.cycles(), 0);
    }
}
