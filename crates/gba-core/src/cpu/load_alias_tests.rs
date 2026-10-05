//! Original regressions for ARM7 single-load base/destination writeback aliases.
use super::*;
use crate::{
    io::{IE, IF, IME, TIMER_BASE},
    machine::{Machine, MachineError, StepKind},
    memory::ROM_START,
    timing::{bus_cycles, AccessKind, AccessWidth},
};

const RAM: u32 = 0x0200_0100;
const DATA: u32 = 0xf233_80a5;

#[derive(Clone, Copy, Debug)]
enum Kind {
    Word,
    Byte,
    Half,
    SignedByte,
    SignedHalf,
}
const KINDS: [Kind; 5] = [
    Kind::Word,
    Kind::Byte,
    Kind::Half,
    Kind::SignedByte,
    Kind::SignedHalf,
];
#[derive(Clone, Copy, Debug)]
enum Index {
    Offset,
    Pre,
    Post,
}

fn load(kind: Kind, index: Index, up: bool, register_offset: bool, register: u32) -> u32 {
    let p = u32::from(!matches!(index, Index::Post)) << 24;
    let w = u32::from(matches!(index, Index::Pre)) << 21;
    let common = 0xe010_0000 | p | w | u32::from(up) << 23 | register << 16 | register << 12;
    match kind {
        Kind::Word | Kind::Byte => {
            common
                | 0x0400_0000
                | u32::from(matches!(kind, Kind::Byte)) << 22
                | if register_offset { 0x0200_0101 } else { 4 }
        } // r1 LSL #2, or #4
        _ => {
            common
                | 0x90
                | match kind {
                    Kind::Half => 0x20,
                    Kind::SignedByte => 0x40,
                    _ => 0x60,
                }
                | if register_offset { 1 } else { 0x0040_0004 }
        } // r1, or split immediate #4
    }
}

fn memory(instruction: u32) -> Memory {
    let mut memory = Memory::new(instruction.to_le_bytes().to_vec()).unwrap();
    memory.write32(RAM, DATA).unwrap();
    memory
}

// Independent byte-lane expectations for [a5,80,33,f2], including odd ARM7 loads.
fn expected(kind: Kind, low: usize) -> u32 {
    (match kind {
        Kind::Word => [0xf233_80a5, 0xa5f2_3380, 0x80a5_f233, 0x3380_a5f2],
        Kind::Byte => [0xa5, 0x80, 0x33, 0xf2],
        Kind::Half => [0x80a5, 0xa500_0080, 0xf233, 0x3300_00f2],
        Kind::SignedByte => [0xffff_ffa5, 0xffff_ff80, 0x33, 0xffff_fff2],
        Kind::SignedHalf => [0xffff_80a5, 0xffff_ff80, 0xffff_f233, 0xffff_fff2],
    })[low]
}

#[test]
fn load_aliases_keep_loaded_values_for_all_widths_offsets_and_index_modes() {
    for kind in KINDS {
        for index in [Index::Offset, Index::Pre, Index::Post] {
            for up in [false, true] {
                for register_offset in [false, true] {
                    for low in 0..4 {
                        let address = RAM + low as u32;
                        let base = match index {
                            Index::Post => address,
                            _ if up => address - 4,
                            _ => address + 4,
                        };
                        let mut bus = memory(load(kind, index, up, register_offset, 0));
                        let mut cpu = Cpu::new(ROM_START);
                        cpu.apply_status(0xf000_00df, Mode::System);
                        cpu.registers[0] = base;
                        cpu.registers[1] = if matches!(kind, Kind::Word | Kind::Byte) {
                            1
                        } else {
                            4
                        };
                        let mut after = cpu.clone();
                        after.registers[0] = expected(kind, low);
                        after.registers[15] += 4;
                        let timing = cpu.step_timed(&mut bus).unwrap();
                        assert_cpu_arch_eq!(cpu, after, "{kind:?} {index:?} up={up} register_offset={register_offset} low={low}");
                        assert_eq!(bus.read32(RAM).unwrap(), DATA);
                        assert_eq!(bus.cycles(), 0);
                        assert_eq!(
                            timing.code_cycles,
                            bus_cycles(0, ROM_START, AccessWidth::Word, AccessKind::Sequential)
                        );
                        assert_eq!(
                            timing.data_cycles,
                            if matches!(kind, Kind::Word) { 6 } else { 3 }
                        );
                        assert_eq!(timing.internal_cycles, 1);
                    }
                }
            }
        }
    }
}

#[test]
fn load_aliases_modify_only_the_active_register_in_every_cpu_bank() {
    let modes = [
        Mode::User,
        Mode::System,
        Mode::Fiq,
        Mode::Irq,
        Mode::Supervisor,
        Mode::Abort,
        Mode::Undefined,
    ];
    for kind in KINDS {
        for mode in modes {
            for register in 0..15 {
                let mut cpu = Cpu::new(ROM_START);
                for bank in modes {
                    cpu.switch_mode(bank);
                    for r in 8..15 {
                        cpu.registers[r] = 0x1000 * bank as u32 + r as u32;
                    }
                }
                cpu.apply_status(0x9000_00c0 | mode as u32, mode);
                cpu.registers[register] = RAM - 4;
                let mut after = cpu.clone();
                after.registers[register] = expected(kind, 0);
                after.registers[15] += 4;
                cpu.step(&mut memory(load(
                    kind,
                    Index::Pre,
                    true,
                    false,
                    register as u32,
                )))
                .unwrap();
                assert_cpu_arch_eq!(cpu, after, "{kind:?} {mode:?} r{register}");
            }
        }
    }
}

#[test]
fn register_offsets_use_incoming_values_and_wrapping_address_arithmetic() {
    for kind in KINDS {
        let mut instruction = load(kind, Index::Pre, true, true, 0);
        if matches!(kind, Kind::Word | Kind::Byte) {
            instruction &= !0x100;
        } // unshifted r1
        let mut bus = memory(instruction);
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[0] = 0xffff_fffc;
        cpu.registers[1] = RAM + 4;
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.registers[0], expected(kind, 0));
        assert_eq!(cpu.registers[1], RAM + 4);
    }
    // All three registers alias: address uses old r0 + (old r0 LSR #24).
    let mut bus = memory(0xe7b0_0c20);
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = RAM;
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], expected(Kind::Word, 2));
}

#[test]
fn post_index_does_not_read_or_validate_the_discarded_writeback_address() {
    for kind in KINDS {
        let mut instruction = load(kind, Index::Post, true, true, 0);
        if matches!(kind, Kind::Word | Kind::Byte) {
            instruction &= !0x100;
        }
        let mut bus = memory(instruction);
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[0] = RAM;
        cpu.registers[1] = 0x7e00_0000; // adjusted address is unmapped 0x80000100
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.registers[0], expected(kind, 0));
    }
}

#[test]
fn failed_alias_reads_preserve_register_banks_device_state_and_clock() {
    for kind in KINDS {
        for index in [Index::Pre, Index::Post] {
            let mut bus = memory(load(kind, index, true, false, 0));
            bus.write32(TIMER_BASE, 0x00c0_fffe).unwrap();
            let mut cpu = Cpu::new(ROM_START);
            cpu.apply_status(0x9000_00d1, Mode::Fiq);
            cpu.registers[0] = 0x0100_0000;
            let before = cpu.clone();
            let display = bus.display_position();
            let counter = bus.read16(TIMER_BASE).unwrap();
            let mut machine = Machine::new(cpu, bus);
            let address = if matches!(index, Index::Pre) {
                0x0100_0004
            } else {
                0x0100_0000
            };
            assert_eq!(
                machine.step(),
                Err(MachineError::Cpu(CpuError::Memory(MemoryError::Unmapped(
                    address
                ))))
            );
            assert_eq!(machine.cpu(), &before);
            assert_eq!(machine.cycles(), 0);
            assert_eq!(machine.last_timing().total(), 0);
            assert_eq!(machine.memory().display_position(), display);
            assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), counter);
            assert_eq!(machine.memory().read16(IF).unwrap(), 0);
            assert_eq!(machine.memory().read32(RAM).unwrap(), DATA);
        }
    }
}

#[test]
fn alias_io_load_reads_before_device_progress_and_irq_delivery() {
    let mut bus = memory(0xe5b0_0004); // LDR r0,[r0,#4]!
    bus.write32(TIMER_BASE, 0x00c0_fffa).unwrap();
    bus.write16(IE, 8).unwrap();
    bus.write16(IME, 1).unwrap();
    let expected = bus.read32(TIMER_BASE).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = TIMER_BASE - 4;
    let mut machine = Machine::new(cpu, bus);
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    assert_eq!(machine.cpu().registers()[0], expected);
    assert_eq!(machine.cpu().pc(), ROM_START + 4);
    assert_eq!(machine.cycles(), 10); // ROM boundary: 8 code + 1 I/O data + 1 internal
    assert_ne!(machine.memory().read32(TIMER_BASE).unwrap(), expected);
    assert_eq!(machine.memory().read16(IF).unwrap(), 8);
    assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
    assert_eq!(machine.cpu().registers()[0], expected);
    assert_eq!(machine.cpu().pc(), 0x18);
}

#[test]
fn skipped_alias_loads_do_not_access_memory_or_apply_writeback() {
    for kind in KINDS {
        for index in [Index::Pre, Index::Post] {
            let mut bus = memory(load(kind, index, true, false, 0) & 0x0fff_ffff); // EQ, Z clear
            let mut cpu = Cpu::new(ROM_START);
            cpu.registers[0] = 0x0100_0000;
            let mut after = cpu.clone();
            after.registers[15] += 4;
            let timing = cpu.step_timed(&mut bus).unwrap();
            assert_cpu_arch_eq!(cpu, after);
            assert_eq!(timing.data_cycles, 0);
            assert_eq!(timing.internal_cycles, 0);
        }
    }
}

#[test]
fn pc_writeback_and_user_transfer_encodings_remain_diagnostics() {
    for kind in KINDS {
        for instruction in [
            load(kind, Index::Pre, true, false, 15),
            load(kind, Index::Post, true, false, 0) | 1 << 21,
        ] {
            let mut cpu = Cpu::new(ROM_START);
            cpu.registers[0] = RAM;
            let before = cpu.clone();
            assert!(matches!(
                cpu.step(&mut memory(instruction)),
                Err(CpuError::UnsupportedInstruction { .. })
            ));
            assert_eq!(cpu, before);
        }
    }
}

#[test]
fn alias_stores_still_store_the_original_base_before_writeback() {
    for kind in [Kind::Word, Kind::Byte, Kind::Half] {
        for index in [Index::Pre, Index::Post] {
            for up in [false, true] {
                let instruction = load(kind, index, up, false, 0) & !(1 << 20);
                let mut bus = memory(instruction);
                let mut cpu = Cpu::new(ROM_START);
                cpu.registers[0] = RAM;
                let adjusted = if up { RAM + 4 } else { RAM - 4 };
                let address = if matches!(index, Index::Pre) {
                    adjusted
                } else {
                    RAM
                };
                cpu.step(&mut bus).unwrap();
                assert_eq!(cpu.registers[0], adjusted);
                match kind {
                    Kind::Word => assert_eq!(bus.read32(address).unwrap(), RAM),
                    Kind::Byte => assert_eq!(bus.read8(address).unwrap(), RAM as u8),
                    Kind::Half => assert_eq!(bus.read16(address).unwrap(), RAM as u16),
                    _ => unreachable!(),
                }
                assert_eq!(cpu.cpsr(), 0x1f);
            }
        }
    }
}
