use gba_core::{
    cpu::{Cpu, Mode},
    display::{CYCLES_PER_FRAME, CYCLES_PER_LINE, HBLANK_START, LINES_PER_FRAME, VBLANK_START},
    io::{DISPCNT, DISPSTAT, IE, IF, IME, TIMER_BASE, VCOUNT},
    machine::{FrameRunError, Machine, StepKind},
    memory::{Memory, MemoryError, BIOS_SIZE, ROM_START},
};

fn memory() -> Memory {
    Memory::new(vec![]).unwrap()
}

#[test]
fn display_starts_at_line_zero_with_comparison_match_but_no_irq() {
    let bus = memory();
    assert_eq!(bus.read16(DISPSTAT).unwrap(), 4);
    assert_eq!(bus.read16(VCOUNT).unwrap(), 0);
    assert_eq!(bus.read16(IF).unwrap(), 0);
    assert_eq!(bus.display_position().scanline, 0);
    assert_eq!(bus.display_position().line_cycle, 0);
    assert_eq!(bus.display_position().frames, 0);
    assert_eq!(bus.display_position().vblanks, 0);
    assert_eq!(bus.cycles(), 0);
}

#[test]
fn hblank_status_starts_at_1006_not_at_the_last_visible_pixel() {
    let mut bus = memory();
    bus.advance_cycles(960);
    assert_eq!(bus.read16(DISPSTAT).unwrap() & 2, 0);
    bus.advance_cycles(45);
    assert_eq!(bus.display_position().line_cycle, 1005);
    assert_eq!(bus.read16(DISPSTAT).unwrap() & 2, 0);
    bus.advance_cycles(1);
    assert_eq!(bus.read16(DISPSTAT).unwrap() & 2, 2);
    bus.advance_cycles(225);
    assert_eq!(bus.read16(VCOUNT).unwrap(), 0);
    assert_eq!(bus.read16(DISPSTAT).unwrap() & 2, 2);
    bus.advance_cycles(1);
    assert_eq!(bus.read16(VCOUNT).unwrap(), 1);
    assert_eq!(bus.display_position().line_cycle, 0);
    assert_eq!(bus.read16(DISPSTAT).unwrap() & 7, 0);
}

#[test]
fn vblank_starts_at_160_clears_at_227_and_line_count_wraps_at_228() {
    let mut bus = memory();
    bus.advance_cycles(VBLANK_START - 1);
    assert_eq!(bus.read16(VCOUNT).unwrap(), 159);
    assert_eq!(bus.read16(DISPSTAT).unwrap() & 3, 2);
    bus.advance_cycles(1);
    assert_eq!(bus.read16(VCOUNT).unwrap(), 160);
    assert_eq!(bus.read16(DISPSTAT).unwrap() & 3, 1);
    assert_eq!(bus.display_position().vblanks, 1);
    bus.advance_cycles(67 * CYCLES_PER_LINE - 1);
    assert_eq!(bus.read16(VCOUNT).unwrap(), 226);
    assert_eq!(bus.read16(DISPSTAT).unwrap() & 3, 3);
    bus.advance_cycles(1);
    assert_eq!(bus.read16(VCOUNT).unwrap(), 227);
    assert_eq!(bus.read16(DISPSTAT).unwrap() & 3, 0);
    assert_eq!(bus.display_position().frames, 0);
    bus.advance_cycles(CYCLES_PER_LINE);
    assert_eq!(bus.read32(DISPSTAT).unwrap(), 4); // Compare zero matches again
    assert_eq!(bus.display_position().frames, 1);
    assert_eq!(bus.display_position().vblanks, 1);
    assert_eq!(bus.cycles(), 280_896);
}

#[test]
fn hblank_irq_occurs_on_every_line_including_hidden_lines() {
    let mut bus = memory();
    bus.write16(DISPSTAT, 0x10).unwrap();
    for line in 0..LINES_PER_FRAME {
        bus.advance_cycles(HBLANK_START - 1);
        assert_eq!(bus.read16(IF).unwrap(), 0);
        bus.advance_cycles(1);
        assert_eq!(bus.read16(VCOUNT).unwrap(), line as u16);
        assert_eq!(bus.read16(IF).unwrap(), 2);
        bus.write16(IF, 2).unwrap();
        bus.advance_cycles(CYCLES_PER_LINE - HBLANK_START);
        assert_eq!(bus.read16(IF).unwrap(), 0);
    }
    assert_eq!(bus.display_position().frames, 1);
}

#[test]
fn vblank_and_compare_requests_latch_without_ie_or_ime_and_acknowledge_independently() {
    let mut bus = memory();
    bus.write16(DISPSTAT, 0xa028).unwrap(); // VBlank + VCount=160
    bus.advance_cycles(VBLANK_START);
    assert_eq!(bus.read16(IF).unwrap(), 5);
    assert!(!bus.irq_pending());
    bus.write16(IE, 5).unwrap();
    assert!(!bus.irq_pending());
    bus.write32(IME, 1).unwrap();
    assert!(bus.irq_pending());
    bus.write16(IF, 1).unwrap();
    assert_eq!(bus.read16(IF).unwrap(), 4);
    assert!(bus.irq_pending());
    bus.write16(IF, 4).unwrap();
    assert!(!bus.irq_pending());
    bus.advance_cycles(0);
    bus.advance_cycles(10);
    assert_eq!(bus.read16(IF).unwrap(), 0);
    bus.advance_cycles(CYCLES_PER_FRAME - 10);
    assert_eq!(bus.read16(IF).unwrap(), 5);
}

#[test]
fn register_masks_read_only_fields_and_word_accesses_are_coherent() {
    let mut bus = memory();
    bus.advance_cycles(17 * CYCLES_PER_LINE + HBLANK_START);
    bus.write32(DISPSTAT, u32::MAX).unwrap();
    assert_eq!(bus.read16(DISPSTAT).unwrap(), 0xff3a);
    assert_eq!(bus.read16(VCOUNT).unwrap(), 17);
    assert_eq!(bus.read32(DISPSTAT).unwrap(), 0x0011_ff3a);
    bus.write8(VCOUNT, 200).unwrap();
    bus.write8(VCOUNT + 1, 0xff).unwrap();
    assert_eq!(bus.read16(VCOUNT).unwrap(), 17);
    bus.write8(DISPSTAT, 0).unwrap();
    assert_eq!(bus.read16(DISPSTAT).unwrap(), 0xff02);
    bus.write8(DISPSTAT + 1, 17).unwrap();
    assert_eq!(bus.read16(DISPSTAT).unwrap(), 0x1106);
    assert_eq!(bus.read8(DISPSTAT).unwrap(), 6);
    assert_eq!(bus.read8(DISPSTAT + 1).unwrap(), 17);
    assert_eq!(bus.read8(VCOUNT + 1).unwrap(), 0);
    assert_eq!(bus.read16(IF).unwrap(), 0);
    assert_eq!(
        bus.write16(DISPSTAT + 1, 0),
        Err(MemoryError::Unaligned(DISPSTAT + 1))
    );
    assert_eq!(bus.read16(DISPSTAT).unwrap(), 0x1106);
}

#[test]
fn writing_comparator_can_create_one_immediate_rising_match_request() {
    let mut bus = memory();
    bus.advance_cycles(7 * CYCLES_PER_LINE);
    bus.write16(DISPSTAT, 0x0620).unwrap();
    assert_eq!(bus.read16(DISPSTAT).unwrap() & 4, 0);
    bus.write8(DISPSTAT + 1, 7).unwrap();
    assert_eq!(bus.read16(DISPSTAT).unwrap() & 4, 4);
    assert_eq!(bus.read16(IF).unwrap(), 4);
    bus.write16(IF, 4).unwrap();
    bus.write8(DISPSTAT + 1, 7).unwrap(); // Already matches
    bus.write8(DISPSTAT, 0).unwrap();
    bus.write8(DISPSTAT, 0x20).unwrap(); // Enabling alone does not create a match edge
    bus.advance_cycles(1);
    assert_eq!(bus.read16(IF).unwrap(), 0);
    bus.write8(DISPSTAT + 1, 6).unwrap();
    bus.write8(DISPSTAT + 1, 7).unwrap();
    assert_eq!(bus.read16(IF).unwrap(), 4);
}

#[test]
fn zero_comparator_irq_waits_for_the_next_line_zero_entry() {
    let mut bus = memory();
    bus.write16(DISPSTAT, 0x20).unwrap();
    assert_eq!(bus.read16(IF).unwrap(), 0);
    bus.advance_cycles(CYCLES_PER_FRAME - 1);
    assert_eq!(bus.read16(IF).unwrap(), 0);
    bus.advance_cycles(1);
    assert_eq!(bus.read16(IF).unwrap(), 4);
    bus.write16(IF, 4).unwrap();
    bus.advance_cycles(1);
    assert_eq!(bus.read16(IF).unwrap(), 0);
}

#[test]
fn out_of_range_comparator_values_never_match() {
    let mut bus = memory();
    for compare in 228..=255 {
        bus.write16(DISPSTAT, (compare << 8) | 0x20).unwrap();
        bus.advance_cycles(CYCLES_PER_FRAME);
        assert_eq!(bus.read16(DISPSTAT).unwrap() & 4, 0);
        assert_eq!(bus.read16(IF).unwrap(), 0);
    }
}

#[test]
fn enabling_vblank_or_hblank_irq_does_not_replay_an_active_status() {
    let mut bus = memory();
    bus.advance_cycles(VBLANK_START);
    bus.write16(DISPSTAT, 8).unwrap();
    bus.advance_cycles(0);
    assert_eq!(bus.read16(IF).unwrap(), 0);
    bus.advance_cycles(CYCLES_PER_FRAME - 1);
    assert_eq!(bus.read16(IF).unwrap(), 0);
    bus.advance_cycles(1);
    assert_eq!(bus.read16(IF).unwrap(), 1);

    let mut bus = memory();
    bus.advance_cycles(HBLANK_START);
    bus.write16(DISPSTAT, 0x10).unwrap();
    bus.advance_cycles(CYCLES_PER_LINE - 1);
    assert_eq!(bus.read16(IF).unwrap(), 0);
    bus.advance_cycles(1);
    assert_eq!(bus.read16(IF).unwrap(), 2);
}

#[test]
fn forced_blank_and_display_mode_changes_do_not_stop_or_reset_the_clock() {
    let mut bus = memory();
    bus.write16(DISPCNT, 0x80).unwrap();
    bus.write16(DISPSTAT, 0x18).unwrap();
    bus.advance_cycles(VBLANK_START);
    let position = bus.display_position();
    assert_eq!(bus.read16(IF).unwrap(), 3);
    bus.write16(DISPCNT, 0x403).unwrap();
    assert_eq!(bus.display_position(), position);
    bus.advance_cycles(CYCLES_PER_FRAME);
    assert_eq!(bus.display_position().vblanks, 2);
    assert_eq!(bus.display_position().scanline, 160);
}

#[test]
fn timer_and_display_events_can_latch_in_the_same_clock_batch() {
    let mut bus = memory();
    let reload = 65536 - HBLANK_START;
    bus.write32(TIMER_BASE, 0x00c0_0000 | reload).unwrap();
    bus.write16(DISPSTAT, 0x10).unwrap();
    bus.advance_cycles(HBLANK_START);
    assert_eq!(bus.read16(IF).unwrap(), 0x0a);
    bus.write16(IF, 2).unwrap();
    assert_eq!(bus.read16(IF).unwrap(), 8);
}

#[test]
fn bulk_clock_advances_match_a_cycle_by_cycle_reference() {
    for compare in [0_u16, 1, 160, 226, 227, 255] {
        let mut bus = memory();
        bus.write16(DISPSTAT, (compare << 8) | 0x38).unwrap();
        let mut line = 0_u16;
        let mut cycle = 0_u16;
        let mut frames = 0_u64;
        let mut vblanks = 0_u64;
        for chunk in [
            0,
            1,
            959,
            46,
            226,
            17_777,
            CYCLES_PER_FRAME * 2 + 99,
            1232,
            77,
        ] {
            let mut pending = 0;
            for _ in 0..chunk {
                cycle += 1;
                if cycle == 1006 {
                    pending |= 2;
                }
                if cycle == 1232 {
                    cycle = 0;
                    line += 1;
                    if line == 228 {
                        line = 0;
                        frames += 1;
                    }
                    if line == 160 {
                        vblanks += 1;
                        pending |= 1;
                    }
                    if line == compare {
                        pending |= 4;
                    }
                }
            }
            bus.advance_cycles(chunk);
            assert_eq!(
                bus.read16(IF).unwrap(),
                pending,
                "compare {compare}, chunk {chunk}"
            );
            let flags = u16::from((160..227).contains(&line))
                | (u16::from(cycle >= 1006) << 1)
                | (u16::from(line == compare) << 2);
            assert_eq!(bus.read16(DISPSTAT).unwrap(), (compare << 8) | 0x38 | flags);
            let position = bus.display_position();
            assert_eq!(
                (
                    position.scanline,
                    position.line_cycle,
                    position.frames,
                    position.vblanks
                ),
                (line, cycle, frames, vblanks)
            );
            bus.write16(IF, 7).unwrap();
        }
    }
}

#[test]
fn maximum_batches_preserve_multi_frame_counts_and_do_not_overflow_u32() {
    let mut bus = memory();
    bus.write16(DISPSTAT, 0xa038).unwrap();
    for multiplier in 1..=2_u64 {
        bus.advance_cycles(u32::MAX);
        let total = multiplier * u64::from(u32::MAX);
        let phase = total % u64::from(CYCLES_PER_FRAME);
        let position = bus.display_position();
        assert_eq!(position.frames, total / u64::from(CYCLES_PER_FRAME));
        assert_eq!(
            position.vblanks,
            (total + u64::from(CYCLES_PER_FRAME - VBLANK_START)) / u64::from(CYCLES_PER_FRAME)
        );
        assert_eq!(
            u64::from(position.scanline),
            phase / u64::from(CYCLES_PER_LINE)
        );
        assert_eq!(
            u64::from(position.line_cycle),
            phase % u64::from(CYCLES_PER_LINE)
        );
        assert_eq!(bus.read16(IF).unwrap(), 7);
        assert_eq!(bus.cycles(), total);
        bus.write16(IF, 7).unwrap();
    }
}

#[test]
fn display_sources_enter_an_original_irq_handler_acknowledge_and_return() {
    for (control, source, edge) in [
        (8, 1, VBLANK_START),
        (0x10, 2, HBLANK_START),
        (0xa020, 4, VBLANK_START),
    ] {
        let mut bios = vec![0; BIOS_SIZE];
        for (index, instruction) in [0xe1c1_20b2_u32, 0xe280_0001, 0xe25e_f004]
            .into_iter()
            .enumerate()
        {
            // STRH r2,[r1,#2] (IF); ADD r0,r0,#1; SUBS pc,lr,#4
            let offset = 0x18 + index * 4;
            bios[offset..offset + 4].copy_from_slice(&instruction.to_le_bytes());
        }
        let rom = [
            0xe3a0_1301_u32,
            0xe281_1c02,
            0xe3a0_2000 | source,
            0xe283_3001,
            0xeaff_fffe,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
        let mut bus = Memory::with_bios(rom, bios).unwrap();
        let mut cpu = Cpu::new(ROM_START);
        for _ in 0..3 {
            cpu.step(&mut bus).unwrap();
        }
        bus.write16(DISPSTAT, control).unwrap();
        bus.write16(IE, source as u16).unwrap();
        bus.write32(IME, 1).unwrap();
        bus.advance_cycles(edge - 1);
        let mut machine = Machine::new(cpu, bus);
        assert_eq!(machine.step().unwrap(), StepKind::Instruction);
        assert_eq!(machine.cpu().registers()[3], 1);
        assert_eq!(machine.memory().read16(IF).unwrap(), source as u16);
        assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
        assert_eq!(machine.cpu().mode(), Mode::Irq);
        assert_eq!(machine.cpu().pc(), 0x18);
        assert_eq!(machine.cpu().registers()[14], ROM_START + 20);
        for _ in 0..3 {
            assert_eq!(machine.step().unwrap(), StepKind::Instruction);
        }
        assert_eq!(machine.cpu().registers()[0], 1);
        assert_eq!(machine.cpu().mode(), Mode::System);
        assert_eq!(machine.cpu().pc(), ROM_START + 16);
        assert_eq!(machine.memory().read16(IF).unwrap(), 0);
    }
}

#[test]
fn failed_block_store_does_not_change_window_controls_or_advance_display() {
    let rom = [
        0xe3a0_2301_u32, // MOV r2,#0x04000000
        0xe282_2048,     // ADD r2,r2,#0x48 (WININ/WINOUT)
        0xe3a0_003f,     // MOV r0,#0x3f
        0xe3a0_1001,     // MOV r1,#1
        0xe8a2_001f,     // STMIA r2!,{r0-r4}; last word is unmapped 0x04000058
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    let mut bus = Memory::new(rom).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    for _ in 0..4 {
        cpu.step(&mut bus).unwrap();
    }
    bus.advance_cycles(7 * CYCLES_PER_LINE);
    let before = cpu.clone();
    let position = bus.display_position();
    let mut machine = Machine::new(cpu, bus);
    assert!(machine.step().is_err());
    assert_eq!(machine.cpu(), &before);
    assert_eq!(machine.memory().display_position(), position);
    assert_eq!(machine.memory().read32(gba_core::io::WININ).unwrap(), 0);
    assert_eq!(machine.memory().read16(DISPSTAT).unwrap(), 0);
    assert_eq!(machine.memory().read16(IF).unwrap(), 0);
}

#[test]
fn frame_runner_waits_for_next_vblank_event_not_an_already_set_flag() {
    let mut bus = memory();
    bus.write32(0x0300_0000, 0xeaff_fffe).unwrap(); // 3-cycle IWRAM self-branch
    bus.advance_cycles(VBLANK_START - 2);
    let mut machine = Machine::new(Cpu::new(0x0300_0000), bus);
    assert_eq!(machine.run_until_vblank(1).unwrap(), 1);
    assert_eq!(machine.memory().display_position().line_cycle, 1);
    assert_eq!(machine.memory().display_position().vblanks, 1);
    let steps = machine
        .run_until_vblank((CYCLES_PER_FRAME / 3) as usize)
        .unwrap();
    assert_eq!(steps, (CYCLES_PER_FRAME / 3) as usize);
    assert_eq!(machine.memory().display_position().line_cycle, 1);
    assert_eq!(machine.memory().display_position().vblanks, 2);
}

#[test]
fn frame_runner_limits_and_cpu_errors_preserve_completed_progress_only() {
    let mut bus = memory();
    bus.write32(0x0300_0000, 0xeaff_fffe).unwrap();
    let mut machine = Machine::new(Cpu::new(0x0300_0000), bus);
    assert_eq!(
        machine.run_until_vblank(0),
        Err(FrameRunError::StepLimit(0))
    );
    assert_eq!(machine.cycles(), 0);
    assert_eq!(
        machine.run_until_vblank(2),
        Err(FrameRunError::StepLimit(2))
    );
    assert_eq!(machine.cycles(), 6);
    assert_eq!(machine.memory().display_position().line_cycle, 6);
    machine
        .memory_mut()
        .write32(0x0300_0000, 0xf000_0000)
        .unwrap();
    let position = machine.memory().display_position();
    assert!(matches!(
        machine.run_until_vblank(10),
        Err(FrameRunError::Cpu(_))
    ));
    // The already buffered branch executes once and refills from the patched word.
    // The unsupported instruction then fails without further device progress.
    assert_eq!(
        machine.memory().display_position().line_cycle,
        position.line_cycle + 3
    );
    assert_eq!(machine.cycles(), 9);
}
