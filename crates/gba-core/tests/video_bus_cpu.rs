//! Original CPU-driven checks that strengthen the public memory ROM's byte tests.
use gba_core::{
    cpu::Cpu,
    io::{DISPCNT, TIMER_BASE},
    machine::{Machine, StepKind},
    memory::{Memory, OAM_START, PALETTE_START, ROM_START, VRAM_START},
    timing::StepTiming,
};

const SENTINEL: u16 = 0xa55a;

fn store_program(address: u32, value: u32, store: u32, control: u16) -> Machine {
    let words = [
        0xe59f_0010, // LDR r0, [pc, #16]: full source value
        0xe59f_1010, // LDR r1, [pc, #16]: destination
        store,
        0xe3c1_3001, // BIC r3, r1, #1: aligned halfword readback address
        0xe1d3_20b0, // LDRH r2, [r3]
        0xeaff_fffe, // B .
        value,
        address,
    ];
    let mut memory = Memory::new(words.into_iter().flat_map(u32::to_le_bytes).collect()).unwrap();
    memory.write16(DISPCNT, control).unwrap();
    memory.write32(TIMER_BASE, 0x0080_0000).unwrap();
    Machine::new(Cpu::new(ROM_START), memory)
}

fn execute_store(machine: &mut Machine, data_cycles: u32) {
    for _ in 0..2 {
        assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    }
    let before = machine.cycles();
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    assert_eq!(
        machine.last_timing(),
        StepTiming {
            code_cycles: 8, // Non-sequential ARM word in ROM window 0.
            data_cycles,
            internal_cycles: 0,
            idle_cycles: 0,
        }
    );
    assert_eq!(machine.cycles() - before, u64::from(8 + data_cycles));
    assert_eq!(
        machine.memory().read16(TIMER_BASE).unwrap(),
        machine.cycles() as u16
    );
    assert_eq!(machine.cpu().pc(), ROM_START + 12);
    assert_eq!(machine.cpu().cpsr(), 0x1f);
}

fn check_byte(address: u32, physical: u32, control: u16, duplicates: bool, value: u32) {
    let mut machine = store_program(address, value, 0xe5c1_0000, control); // STRB r0, [r1]
    let target = physical & !1;
    machine.memory_mut().write16(target - 2, 0x1357).unwrap();
    machine.memory_mut().write16(target, SENTINEL).unwrap();
    machine.memory_mut().write16(target + 2, 0x2468).unwrap();
    execute_store(&mut machine, 1); // Ignored stores still consume their data access.
    let expected = if duplicates {
        (value as u8 as u16) * 0x101
    } else {
        SENTINEL
    };
    assert_eq!(
        machine.memory().read16(target).unwrap(),
        expected,
        "address={address:#010x} control={control:#06x} value={value:#010x}"
    );
    assert_eq!(machine.memory().read16(address & !1).unwrap(), expected);
    assert_eq!(machine.memory().read16(target - 2).unwrap(), 0x1357);
    assert_eq!(machine.memory().read16(target + 2).unwrap(), 0x2468);
    for _ in 0..2 {
        assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    }
    assert_eq!(machine.cpu().registers()[0], value);
    assert_eq!(machine.cpu().registers()[1], address);
    assert_eq!(machine.cpu().registers()[2], u32::from(expected));
    assert_eq!(machine.cpu().cpsr(), 0x1f);
    assert_eq!(machine.memory().read16(DISPCNT).unwrap(), control);
}

#[test]
fn cpu_strb_obeys_exact_vram_boundaries_in_every_supported_mode_and_mirror() {
    // Explicit physical addresses and allowed mode classes, independent of vram_index.
    // The final flag marks the 64..80 KiB area that accepts bytes only in bitmap modes.
    let cases = [
        (0x0600_0020, 0x0600_0020, true, false),
        (0x0600_fffe, 0x0600_fffe, true, false),
        (0x0601_0000, 0x0601_0000, false, true),
        (0x0601_3ffe, 0x0601_3ffe, false, true),
        (0x0601_4000, 0x0601_4000, false, false),
        (0x0601_7ffe, 0x0601_7ffe, false, false),
        (0x0601_8000, 0x0601_0000, false, true),
        (0x0601_bffe, 0x0601_3ffe, false, true),
        (0x0601_c000, 0x0601_4000, false, false),
        (0x0601_fffe, 0x0601_7ffe, false, false),
        (0x0602_0020, 0x0600_0020, true, false),
        (0x06ff_fffe, 0x0601_7ffe, false, false),
    ];
    for mode in 0..=5 {
        for blank in [0, 0x80] {
            for (address, physical, always, bitmap_only) in cases {
                for low in 0..2 {
                    for value in [0x1234_5600, 0xaabb_cc01, 0x9988_77a5, 0xffff_ffff] {
                        check_byte(
                            address + low,
                            physical + low,
                            mode | blank,
                            always || (bitmap_only && mode >= 3),
                            value,
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn cpu_strb_duplicates_palette_but_preserves_every_oam_byte() {
    for (address, physical, duplicates) in [
        (PALETTE_START + 0x20, PALETTE_START + 0x20, true),
        (0x0500_0420, PALETTE_START + 0x20, true),
        (0x05ff_fffe, PALETTE_START + 0x3fe, true),
        (OAM_START + 0x20, OAM_START + 0x20, false),
        (0x0700_0420, OAM_START + 0x20, false),
        (0x07ff_fffe, OAM_START + 0x3fe, false),
    ] {
        for mode in 0..=5 {
            for low in 0..2 {
                for value in [0, 1, 0x7f, 0x80, 0xff] {
                    check_byte(address + low, physical + low, mode, duplicates, value);
                }
            }
        }
    }
}

#[test]
fn cpu_halfword_and_word_stores_still_modify_obj_and_oam() {
    for mode in 0..=5 {
        for address in [VRAM_START + 0x10000, VRAM_START + 0x14000, OAM_START + 0x20] {
            for (store, word) in [(0xe1c1_00b0, false), (0xe581_0000, true)] {
                // STRH / STR
                let mut machine = store_program(address, 0x1234_5678, store, mode);
                machine.memory_mut().write32(address, 0xa55a_aa55).unwrap();
                let data_cycles = if word && address < OAM_START { 2 } else { 1 };
                execute_store(&mut machine, data_cycles);
                assert_eq!(
                    machine.memory().read32(address).unwrap(),
                    if word { 0x1234_5678 } else { 0xa55a_5678 }
                );
            }
        }
    }
}
