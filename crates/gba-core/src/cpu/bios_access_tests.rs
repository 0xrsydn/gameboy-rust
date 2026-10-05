//! Original synthetic BIOS images; no Nintendo firmware or expected firmware constants.
use super::*;
use crate::{
    io::TIMER_BASE,
    machine::{Machine, StepKind},
    memory::{BIOS_SIZE, ROM_START},
    timing::StepTiming,
};

#[path = "thumb_bios_access_tests.rs"]
mod thumb;

const ENTRY: u32 = 0x100;
const DATA: u32 = 0xf233_80a5;
const OTHER: u32 = 0x1234_5678;
const LDR: u32 = 0xe590_1000; // LDR r1, [r0]
const BX_R4: u32 = 0xe12f_ff14;

fn word(bytes: &mut [u8], address: u32, value: u32) {
    bytes[address as usize..address as usize + 4].copy_from_slice(&value.to_le_bytes());
}

fn image() -> Vec<u8> {
    let mut bios = vec![0x55; BIOS_SIZE];
    word(&mut bios, ENTRY, BX_R4);
    word(&mut bios, ENTRY + 8, DATA);
    word(&mut bios, 0x200, BX_R4);
    word(&mut bios, 0x208, OTHER);
    bios
}

fn bus(instruction: u32, bios: Vec<u8>) -> Memory {
    let rom = [instruction, LDR, OTHER, OTHER];
    Memory::with_bios(rom.into_iter().flat_map(u32::to_le_bytes).collect(), bios).unwrap()
}

fn leave_bios(memory: &mut Memory, thumb: bool) -> Cpu {
    let mut cpu = Cpu::new(ENTRY);
    cpu.registers[4] = ROM_START | u32::from(thumb);
    cpu.step(memory).unwrap();
    assert_eq!(cpu.pc(), ROM_START);
    cpu
}

#[test]
fn protected_bios_loads_use_latched_lanes_for_arm_and_thumb_callers() {
    for (arm, thumb, values) in [
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
        for is_thumb in [false, true] {
            for address in [0, ENTRY, 0x3ffc] {
                for (low, value) in values.into_iter().enumerate() {
                    let mut memory = bus(if is_thumb { thumb } else { arm }, image());
                    let mut cpu = leave_bios(&mut memory, is_thumb);
                    cpu.registers[0] = address + low as u32;
                    cpu.flags.carry = true;
                    let mut expected = cpu.clone();
                    expected.registers[1] = value;
                    expected.registers[15] += if is_thumb { 2 } else { 4 };
                    let timing = cpu.step_timed(&mut memory).unwrap();
                    assert_eq!(
                        cpu, expected,
                        "arm={arm:#010x} thumb={is_thumb} address={address:#x} lane={low}"
                    );
                    assert_eq!(timing.data_cycles, 1);
                    assert_eq!(timing.internal_cycles, 1);
                    assert_eq!(memory.cycles(), 0);
                }
            }
        }
    }
}

#[test]
fn protection_depends_on_executing_address_not_processor_mode() {
    for mode in [
        Mode::User,
        Mode::System,
        Mode::Fiq,
        Mode::Irq,
        Mode::Supervisor,
        Mode::Abort,
        Mode::Undefined,
    ] {
        for thumb in [false, true] {
            let mut bios = image();
            word(&mut bios, 0x200, if thumb { 0x6801 } else { LDR });
            word(&mut bios, 0x300, OTHER);
            let mut memory = bus(if thumb { 0x6801 } else { LDR }, bios);
            let mut cpu = leave_bios(&mut memory, thumb);
            cpu.apply_status(0xb000_00c0 | (u32::from(thumb) << 5) | mode as u32, mode);
            cpu.registers[0] = 0x300;
            cpu.step(&mut memory).unwrap(); // Outside BIOS: even privileged modes cannot read raw bytes.
            assert_eq!(cpu.registers[1], DATA);
            cpu.registers[15] = 0x200;
            cpu.step(&mut memory).unwrap(); // Inside BIOS: both instruction sets can read raw data.
            assert_eq!(cpu.registers[1], OTHER);
            assert_eq!(
                cpu.cpsr(),
                0xb000_00c0 | (u32::from(thumb) << 5) | mode as u32
            );
        }
    }
}

#[test]
fn host_inspection_does_not_change_the_latch_or_expose_cpu_protected_bytes() {
    let mut memory = bus(LDR, image());
    let mut cpu = leave_bios(&mut memory, false);
    for address in [0, 0x200, 0x3ffc] {
        assert_eq!(
            memory.read32(address).unwrap(),
            if address == 0x200 { BX_R4 } else { 0x5555_5555 }
        );
    }
    assert_eq!(memory.read32(ENTRY + 8).unwrap(), DATA);
    assert_eq!(memory.read32(0x208).unwrap(), OTHER);
    assert_eq!(memory.write32(0, 0), Err(MemoryError::ReadOnly(0)));
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[1], DATA);
    cpu.step(&mut memory).unwrap(); // More ROM execution must not replace the BIOS latch.
    assert_eq!(cpu.registers[1], DATA);
}

#[test]
fn bios_reentry_and_exception_returns_refresh_the_image_derived_value() {
    let mut memory = bus(LDR, image());
    let mut cpu = leave_bios(&mut memory, false);
    cpu.registers[15] = 0x200;
    cpu.step(&mut memory).unwrap();
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[1], OTHER);

    for swi in [false, true] {
        let mut bios = image();
        let vector = if swi { 8 } else { 0x18 };
        word(
            &mut bios,
            vector,
            if swi { 0xe1b0_f00e } else { 0xe25e_f004 },
        ); // MOVS/SUBS pc,lr
        word(&mut bios, vector + 8, OTHER);
        let mut memory = bus(if swi { 0xef00_0000 } else { LDR }, bios);
        let mut cpu = leave_bios(&mut memory, false);
        if swi {
            cpu.step(&mut memory).unwrap();
        } else {
            cpu.enter_exception(Exception::Irq);
        }
        assert_eq!(cpu.pc(), vector);
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.mode(), Mode::System);
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[1], OTHER);
    }
}

#[test]
fn unknown_bios_history_returns_a_diagnostic_not_raw_bytes_or_a_magic_value() {
    for bios in [false, true] {
        let mut memory = if bios {
            bus(LDR, image())
        } else {
            Memory::new(LDR.to_le_bytes().to_vec()).unwrap()
        };
        if bios {
            assert_eq!(memory.read32(0).unwrap(), 0x5555_5555);
        }
        let mut cpu = Cpu::new(ROM_START);
        let before = cpu.clone();
        assert_eq!(
            cpu.step(&mut memory),
            Err(CpuError::Memory(MemoryError::Unmapped(0)))
        );
        assert_eq!(cpu, before);
    }
}

#[test]
fn diagnostic_steps_keep_previous_bios_history_but_skipped_instructions_refresh_it() {
    for instruction in [0xe7f0_00f0, LDR] {
        let mut bios = image();
        word(&mut bios, 0x200, instruction);
        let mut memory = bus(LDR, bios);
        let mut cpu = leave_bios(&mut memory, false);
        cpu.registers[15] = 0x200;
        cpu.registers[0] = 0x0e00_0000;
        let before = cpu.clone();
        assert!(cpu.step_timed(&mut memory).is_err());
        assert_eq!(cpu, before);
        cpu.registers[15] = ROM_START;
        cpu.registers[0] = 0;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[1], DATA);
    }
    let mut bios = image();
    word(&mut bios, 0x200, 0x0590_1000); // LDREQ skipped with Z clear.
    let mut memory = bus(LDR, bios);
    let mut cpu = leave_bios(&mut memory, false);
    cpu.registers[15] = 0x200;
    cpu.step(&mut memory).unwrap();
    cpu.registers[15] = ROM_START;
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[1], OTHER);
}

#[test]
fn missing_arm_or_thumb_lookahead_invalidates_unsupported_history() {
    for (pc, thumb) in [(0x3ffc, true), (0x3ff8, false), (0x3ffc, false)] {
        let mut bios = image();
        word(&mut bios, pc, if thumb { 0x4720 } else { BX_R4 }); // Thumb BX r4 or ARM BX r4
        let mut memory = bus(LDR, bios);
        let mut cpu = leave_bios(&mut memory, false);
        cpu.registers[15] = pc;
        cpu.instruction_set = if thumb {
            InstructionSet::Thumb
        } else {
            InstructionSet::Arm
        };
        cpu.step(&mut memory).unwrap();
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
fn bios_block_loads_share_the_latch_but_unused_memory_uses_rom_prefetch() {
    let mut memory = bus(0xe8b0_000e, image()); // LDMIA r0!, {r1-r3}
    let mut cpu = leave_bios(&mut memory, false);
    cpu.step(&mut memory).unwrap();
    assert_eq!(&cpu.registers[1..4], &[DATA; 3]);
    assert_eq!(cpu.registers[0], 12);

    let mut memory = bus(LDR, image());
    let mut cpu = leave_bios(&mut memory, false);
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[1], DATA);
    cpu.registers[0] = 0x4000;
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[1], OTHER);
}

#[test]
fn original_boot_protected_read_matches_its_actual_last_bios_snapshot() {
    let rom = [0xe3a0_0000, LDR]; // MOV r0,#0; LDR r1,[r0]
    let mut machine =
        crate::bios::boot(rom.into_iter().flat_map(u32::to_le_bytes).collect()).unwrap();
    let bios = crate::bios::image();
    let mut last_bios_pc = None;
    for _ in 0..128 {
        if machine.cpu().pc() == ROM_START {
            break;
        }
        let pc = machine.cpu().pc();
        assert!(pc < BIOS_SIZE as u32);
        assert_eq!(machine.step().unwrap(), StepKind::Instruction);
        last_bios_pc = Some(pc);
    }
    assert_eq!(machine.cpu().pc(), ROM_START);
    let offset = last_bios_pc.unwrap() as usize + 8;
    let expected = u32::from_le_bytes(bios[offset..offset + 4].try_into().unwrap());
    assert_ne!(expected, machine.memory().read32(0).unwrap());
    machine.step().unwrap();
    machine.step().unwrap();
    assert_eq!(machine.cpu().registers()[1], expected);
}

#[test]
fn protected_reads_charge_one_data_access_before_device_progress() {
    let mut memory = bus(LDR, image());
    let cpu = leave_bios(&mut memory, false);
    memory.write32(TIMER_BASE, 0x0080_0000).unwrap();
    let mut machine = Machine::new(cpu, memory);
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    assert_eq!(machine.cpu().registers()[1], DATA);
    assert_eq!(
        machine.last_timing(),
        StepTiming {
            code_cycles: 8,
            data_cycles: 1,
            internal_cycles: 1,
            idle_cycles: 0
        }
    );
    assert_eq!(machine.cycles(), 10);
    assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), 10);
}
