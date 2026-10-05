//! Original sequential Thumb programs for the lane-retaining IWRAM bus.
use super::*;
use crate::{
    dma::{DMA_BASE, DMA_STRIDE},
    io::{IE, IME, TIMER_BASE},
    machine::{Machine, StepKind},
    memory::ROM_START,
};

const CODE: u32 = 0x0300_0100;
const SOURCE: u32 = 0x0300_1000;
const OTHER: u32 = 0x0300_2000;
const UNUSED: u32 = 0x8000_0000;
const DATA: u32 = 0xf233_80a5;
const NEW_DATA: u32 = 0x7654_fedc;
const NOP: u16 = 0x46c0;
const SEED: u16 = 0x680a; // LDR r2,[r1]
const PROBE: u16 = 0x6806; // LDR r6,[r0]

fn program(pc: u32, code: &[u16]) -> (Cpu, Memory) {
    let mut bus = Memory::new(NEW_DATA.to_le_bytes().to_vec()).unwrap();
    for (index, instruction) in code.iter().enumerate() {
        bus.write16(pc + index as u32 * 2, *instruction).unwrap();
    }
    bus.write32(SOURCE, DATA).unwrap();
    bus.write32(OTHER, NEW_DATA).unwrap();
    let mut cpu = Cpu::new(pc);
    cpu.instruction_set = InstructionSet::Thumb;
    cpu.registers[0] = UNUSED;
    cpu.registers[1] = SOURCE;
    cpu.registers[4] = OTHER;
    (cpu, bus)
}

// Independent byte-array reference: an IWRAM access replaces only addressed bytes.
fn drive(word: u32, address: u32, bytes: &[u8]) -> u32 {
    let mut lanes = word.to_le_bytes();
    let lane = (address & 3) as usize;
    lanes[lane..lane + bytes.len()].copy_from_slice(bytes);
    u32::from_le_bytes(lanes)
}

#[test]
fn sequential_fetches_fill_alternating_lanes_and_keep_the_observed_old_halfword() {
    for pc in [
        CODE,
        CODE + 2,
        CODE + 0x8000,
        CODE + 0x8002,
        0x0300_7ffc,
        0x0300_7ffe,
    ] {
        let (mut cpu, mut bus) = program(pc, &[NOP, PROBE, 0x1234, 0xabcd]);
        cpu.step(&mut bus).unwrap();
        // Host reads and writes do not drive the emulated bus. Do not reread P+2 as history.
        assert_eq!(bus.read16(pc + 4).unwrap(), 0x1234);
        bus.write16(pc + 4, 0xeeee).unwrap();
        bus.write32(OTHER, 0xeeee_eeee).unwrap();
        cpu.step(&mut bus).unwrap();
        assert_eq!(
            cpu.registers[6],
            if pc & 2 == 0 {
                0xabcd_1234
            } else {
                0x1234_abcd
            }
        );
    }
}

#[test]
fn word_load_seeds_history_before_cpu_rotation_and_current_fetch_replaces_one_half() {
    for pc in [CODE, CODE + 2] {
        for low in 0..4 {
            let (mut cpu, mut bus) = program(pc, &[SEED, PROBE, 0x1111, 0x9bcd]);
            cpu.registers[1] += low;
            cpu.step(&mut bus).unwrap();
            assert_eq!(cpu.registers[2], DATA.rotate_right(low * 8));
            cpu.step(&mut bus).unwrap();
            assert_eq!(
                cpu.registers[6],
                if pc & 2 == 0 {
                    0x9bcd_80a5
                } else {
                    0xf233_9bcd
                }
            );
        }
    }
}

#[test]
fn known_history_supports_widths_sign_extension_and_unaligned_loads_in_all_modes() {
    for mode in [
        Mode::User,
        Mode::System,
        Mode::Fiq,
        Mode::Irq,
        Mode::Supervisor,
        Mode::Abort,
        Mode::Undefined,
    ] {
        for pc in [CODE, CODE + 2] {
            let word: u32 = if pc & 2 == 0 {
                0x9bcd_80a5
            } else {
                0xf233_9bcd
            };
            for (instruction, width, signed) in [
                (0x6806, 4, false),
                (0x7806, 1, false),
                (0x8806, 2, false),
                (0x5746, 1, true),
                (0x5f46, 2, true), // r5=0 offset, r0 base, r6 result.
            ] {
                for lane in 0..4 {
                    let (mut cpu, mut bus) = program(pc, &[SEED, instruction, 0x1111, 0x9bcd]);
                    cpu.apply_status(0xb000_00e0 | mode as u32, mode);
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
                    let mut expected = cpu.clone();
                    expected.registers[6] = value;
                    expected.registers[15] += 2;
                    cpu.step(&mut bus).unwrap();
                    assert_cpu_arch_eq!(cpu, expected);
                }
            }
        }
    }
}

#[test]
fn byte_halfword_and_word_iwram_data_accesses_replace_only_addressed_lanes() {
    for pc in [CODE, CODE + 2] {
        for (instruction, width, store) in [
            (0x7823, 1, false),
            (0x8823, 2, false),
            (0x6823, 4, false),
            (0x5763, 1, false),
            (0x5f63, 2, false), // Signed loads, offset r5=0.
            (0x7023, 1, true),
            (0x8023, 2, true),
            (0x6023, 4, true),
        ] {
            for lane in 0..4 {
                let (mut cpu, mut bus) = program(pc, &[SEED, instruction, PROBE, 0x5566, 0x99aa]);
                cpu.registers[3] = 0x1234_abcd;
                cpu.registers[4] += lane;
                cpu.step(&mut bus).unwrap();
                let mut word = drive(DATA, pc + 6, &0x5566_u16.to_le_bytes());
                let address = (OTHER + lane) & !(width - 1);
                let start = (address & 3) as usize;
                let value = if store {
                    0x1234_abcd_u32.to_le_bytes()
                } else {
                    NEW_DATA.to_le_bytes()
                };
                // Odd LDRSH uses a byte, not a halfword, on ARM7.
                let (address, start, width) = if instruction == 0x5f63 && lane & 1 != 0 {
                    (OTHER + lane, lane as usize, 1)
                } else {
                    (address, start, width)
                };
                let bytes = if store {
                    &value[..width as usize]
                } else {
                    &value[start..start + width as usize]
                };
                word = drive(word, address, bytes);
                cpu.step(&mut bus).unwrap();
                word = drive(word, pc + 8, &0x99aa_u16.to_le_bytes());
                cpu.step(&mut bus).unwrap();
                assert_eq!(
                    cpu.registers[6], word,
                    "pc={pc:#x} op={instruction:#x} lane={lane}"
                );
            }
        }
    }
}

#[test]
fn non_iwram_data_reads_and_writes_do_not_replace_the_iwram_latch() {
    for pc in [CODE, CODE + 2] {
        for address in [
            0x0200_0000,
            0x0500_0000,
            0x0600_0000,
            0x0700_0000,
            ROM_START,
        ] {
            for instruction in [0x7823, 0x8823, 0x6823, 0x7023, 0x8023, 0x6023] {
                if address == ROM_START && instruction & 0x0800 == 0 {
                    continue;
                }
                let (mut cpu, mut bus) = program(pc, &[SEED, instruction, PROBE, 0x5566, 0x99aa]);
                if address != ROM_START {
                    bus.write32(address, NEW_DATA).unwrap();
                }
                cpu.registers[4] = address;
                cpu.registers[3] = NEW_DATA;
                cpu.step(&mut bus).unwrap();
                cpu.step(&mut bus).unwrap();
                cpu.step(&mut bus).unwrap();
                let word = drive(
                    drive(DATA, pc + 6, &0x5566_u16.to_le_bytes()),
                    pc + 8,
                    &0x99aa_u16.to_le_bytes(),
                );
                assert_eq!(
                    cpu.registers[6], word,
                    "address={address:#x} op={instruction:#x}"
                );
            }
        }
    }
}

#[test]
fn block_transfers_leave_the_last_iwram_word_and_keep_writeback() {
    for pc in [CODE, CODE + 2] {
        for store in [false, true] {
            let instruction = if store { 0xc40e } else { 0xcc0e }; // STM/LDM r4!,{r1-r3}
            let (mut cpu, mut bus) = program(pc, &[instruction, PROBE, 0x1111, 0x9bcd]);
            for (index, value) in [DATA, NEW_DATA, 0x1357_2468].into_iter().enumerate() {
                bus.write32(OTHER + index as u32 * 4, value).unwrap();
                cpu.registers[index + 1] = value;
            }
            cpu.step(&mut bus).unwrap();
            assert_eq!(cpu.registers[4], OTHER + 12);
            cpu.step(&mut bus).unwrap();
            assert_eq!(
                cpu.registers[6],
                drive(0x1357_2468, pc + 6, &0x9bcd_u16.to_le_bytes())
            );
        }
    }
}

#[test]
fn cold_history_is_unknown_but_successful_fetches_or_word_accesses_can_establish_it() {
    for pc in [CODE, CODE + 2] {
        let (mut cpu, mut bus) = program(pc, &[PROBE, PROBE, 0x1111, 0x2222]);
        let before = cpu.clone();
        assert_eq!(
            cpu.step(&mut bus),
            Err(CpuError::Memory(MemoryError::Unmapped(UNUSED)))
        );
        assert_eq!(cpu, before);
        bus.write16(pc, NOP).unwrap();
        cpu.step(&mut bus).unwrap();
        cpu.step(&mut bus).unwrap();
        assert_eq!(
            cpu.registers[6],
            if pc & 2 == 0 {
                0x2222_1111
            } else {
                0x1111_2222
            }
        );
    }
}

#[test]
fn failed_steps_discard_prefetch_history_before_retry() {
    for pc in [CODE, CODE + 2] {
        for instruction in [0xde00, 0x6823] {
            // Undefined instruction / LDR from unsupported save memory.
            let (mut cpu, mut bus) = program(pc, &[SEED, instruction, 0x1111, 0x9bcd]);
            cpu.registers[4] = 0x0e00_0000;
            cpu.step(&mut bus).unwrap();
            let before = cpu.clone();
            assert!(cpu.step_timed(&mut bus).is_err());
            assert_eq!(cpu, before);
            bus.write16(pc + 2, PROBE).unwrap();
            cpu.invalidate_pipeline(); // Debugger repair leaves separate bus history intact.
            cpu.step(&mut bus).unwrap();
            assert_eq!(
                cpu.registers[6],
                drive(DATA, pc + 6, &0x9bcd_u16.to_le_bytes())
            );
            assert_eq!(bus.cycles(), 0);
        }
    }
}

#[test]
fn taken_control_flow_refills_history_even_when_target_is_fallthrough() {
    for branch in [0x4720, 0xe7ff, 0x46a7, 0xd0ff, 0xbd00] {
        // BX, B, MOV pc, BEQ, POP pc.
        let (mut cpu, mut bus) = program(CODE, &[SEED, branch, PROBE, 0x5566, 0x99aa]);
        cpu.registers[4] = (CODE + 4) | 1;
        cpu.registers[13] = OTHER;
        bus.write32(OTHER, CODE + 4).unwrap();
        cpu.flags.zero = true;
        cpu.step(&mut bus).unwrap();
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.pc(), CODE + 4);
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.registers[6], 0x5566_99aa);
    }
    let (mut cpu, mut bus) = program(CODE, &[SEED, 0xd0ff, PROBE, 0x5566, 0x99aa]);
    cpu.flags.zero = false; // Untaken BEQ has no refill.
    for _ in 0..3 {
        cpu.step(&mut bus).unwrap();
    }
    assert_eq!(cpu.registers[6], 0x5566_99aa);
}

#[test]
fn pc_discontinuities_and_non_iwram_execution_do_not_reuse_old_history() {
    let (mut cpu, mut bus) = program(CODE, &[SEED, PROBE, 0x1111, 0x2222]);
    cpu.step(&mut bus).unwrap();
    bus.write16(CODE + 0x100, PROBE).unwrap();
    cpu.registers[15] = CODE + 0x100;
    assert_eq!(
        cpu.step(&mut bus),
        Err(CpuError::Memory(MemoryError::Unmapped(UNUSED)))
    );
    bus.write16(0x0200_0000, NOP).unwrap();
    cpu.registers[15] = 0x0200_0000;
    cpu.step(&mut bus).unwrap();
    cpu.registers[15] = CODE + 2;
    assert_eq!(
        cpu.step(&mut bus),
        Err(CpuError::Memory(MemoryError::Unmapped(UNUSED)))
    );
}

#[test]
fn successful_dma_updates_history_and_failed_dma_preserves_it() {
    for fail in [false, true] {
        let (cpu, mut bus) = program(CODE, &[SEED, PROBE, 0x1111, 0x9bcd]);
        bus.write32(TIMER_BASE, 0x0080_0000).unwrap();
        let mut machine = Machine::new(cpu, bus);
        machine.step().unwrap();
        let cycles = machine.cycles();
        let before = machine.cpu().clone();
        let dma = DMA_BASE + 3 * DMA_STRIDE;
        machine
            .memory_mut()
            .write32(dma, if fail { 0x0e00_0000 } else { SOURCE })
            .unwrap();
        machine.memory_mut().write32(dma + 4, 0x0200_0000).unwrap();
        machine.memory_mut().write32(dma + 8, 0x8400_0001).unwrap();
        if fail {
            assert!(machine.step().is_err());
            assert_eq!(machine.cycles(), cycles);
            machine.memory_mut().write16(dma + 10, 0).unwrap();
            machine.step().unwrap();
            assert_eq!(machine.cpu().registers()[6], 0x9bcd_80a5);
        } else {
            assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 3 });
            assert_eq!(machine.cpu(), &before);
            assert_eq!(machine.memory().read32(0x0200_0000).unwrap(), DATA);
            let cycles = machine.cycles();
            assert_eq!(machine.step().unwrap(), StepKind::Instruction);
            assert_eq!(machine.cpu().registers()[6], 0x9bcd_80a5);
            assert_eq!(machine.cycles(), cycles + 3);
        }
        assert_eq!(
            u64::from(machine.memory().read16(TIMER_BASE).unwrap()),
            machine.cycles()
        );
    }
}

#[test]
fn history_updates_add_no_data_accesses_or_device_cycles() {
    for pc in [CODE, CODE + 2] {
        let code = [SEED, PROBE, 0x1111, 0x9bcd];
        let (cpu, mut bus) = program(pc, &code);
        bus.write32(TIMER_BASE, 0x0080_0000).unwrap();
        let mut machine = Machine::new(cpu, bus);
        let (mut untimed, mut bus) = program(pc, &code);
        for _ in 0..2 {
            untimed.step(&mut bus).unwrap();
            assert_eq!(machine.step().unwrap(), StepKind::Instruction);
            assert_eq!(machine.cpu(), &untimed);
            assert_eq!(machine.last_timing().code_cycles, 1);
            assert_eq!(machine.last_timing().data_cycles, 1);
            assert_eq!(machine.last_timing().internal_cycles, 1);
        }
        assert_eq!(machine.cycles(), 6);
        assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), 6);
        assert_eq!(bus.cycles(), 0);
    }
}

#[test]
fn accepted_irq_invalidates_history_before_any_handler_instruction() {
    let (cpu, bus) = program(CODE, &[SEED, PROBE, 0x1111, 0x9bcd]);
    let mut machine = Machine::new(cpu, bus);
    machine.step().unwrap();
    let mut interrupted = machine.cpu().clone();
    machine.memory_mut().write16(IE, 8).unwrap();
    machine.memory_mut().write16(IME, 1).unwrap();
    machine
        .memory_mut()
        .write32(TIMER_BASE, 0x00c0_ffff)
        .unwrap();
    machine.memory_mut().advance_cycles(1);
    assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
    // Probe the paused instruction with a separate CPU, without executing BIOS.
    assert_eq!(
        interrupted.step(machine.memory_mut()),
        Err(CpuError::Memory(MemoryError::Unmapped(UNUSED)))
    );
}
