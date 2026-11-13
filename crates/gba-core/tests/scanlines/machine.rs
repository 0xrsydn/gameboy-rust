use super::*;

#[test]
fn hblank_dma_updates_next_row_while_cpu_stays_halted() {
    let mut m = memory();
    m.write16(PAL, 31).unwrap();
    for i in 0..160 {
        m.write16(0x0200_0000 + i * 2, ((i + 1) as u16 & 31) << 5)
            .unwrap();
    }
    m.write32(DMA_BASE, 0x0200_0000).unwrap();
    m.write32(DMA_BASE + 4, PAL).unwrap();
    m.write32(DMA_BASE + 8, 0xa240_0001).unwrap();
    m.write8(HALTCNT, 0).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), m);
    let cpu = machine.cpu().clone();
    assert_eq!(machine.step().unwrap(), StepKind::HaltIdle);
    assert_eq!(
        machine.memory().display_position().line_cycle,
        HBLANK_START as u16
    );
    assert_eq!(machine.step().unwrap(), StepKind::HaltIdle); // The transfer's start delay.
    assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 0 });
    machine.run_until_vblank(1000).unwrap();
    assert_eq!(machine.cpu(), &cpu);
    assert!(machine.halted());
    let f = present(machine.memory());
    for y in 0..HEIGHT {
        let color = if y == 0 { 31 } else { (y as u16 & 31) << 5 };
        assert!(f.pixels()[y * WIDTH..(y + 1) * WIDTH]
            .iter()
            .all(|&p| p == rgb(color)));
    }
    assert_eq!(machine.memory().read16(PAL).unwrap(), 0); // Row159 HBlank DMA ran after its capture.
}

#[test]
fn vblank_dma_cannot_change_the_frame_just_published() {
    let mut m = memory();
    m.write16(PAL, 31).unwrap();
    m.write16(0x0200_0000, 0x3e0).unwrap();
    m.write32(DMA_BASE, 0x0200_0000).unwrap();
    m.write32(DMA_BASE + 4, PAL).unwrap();
    m.write32(DMA_BASE + 8, 0x9000_0001).unwrap();
    m.advance_cycles(VBLANK_START + 3); // The edge, then the transfer's start delay.
    let mut machine = Machine::new(Cpu::new(ROM_START), m);
    assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 0 });
    assert!(present(machine.memory())
        .pixels()
        .iter()
        .all(|&p| p == rgb(31)));
    machine.memory_mut().advance_cycles(CYCLES_PER_FRAME);
    assert!(present(machine.memory())
        .pixels()
        .iter()
        .all(|&p| p == rgb(0x3e0)));
}

#[test]
fn cpu_polling_vcount_can_split_the_frame_without_host_video_writes() {
    let rom = word_rom(&[
        0xe3a0_0301, // MOV r0,#0x04000000
        0xe1d0_10b6, // LDRH r1,[r0,#6] (VCOUNT)
        0xe351_0050, // CMP r1,#80
        0x1aff_fffc, // BNE poll
        0xe3a0_2405, // MOV r2,#0x05000000
        0xe3a0_3c7c, // MOV r3,#0x7c00
        0xe1c2_30b0, // STRH r3,[r2]
        0xeaff_fffe,
    ]);
    let mut m = Memory::new(rom).unwrap();
    m.set_scanline_rendering(true);
    m.write16(PAL, 31).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), m);
    machine.run_until_vblank(100_000).unwrap();
    let f = present(machine.memory());
    for y in 0..HEIGHT {
        let color = if y < 80 { 31 } else { 0x7c00 };
        assert!(f.pixels()[y * WIDTH..(y + 1) * WIDTH]
            .iter()
            .all(|&p| p == rgb(color)));
    }
}

#[test]
fn instruction_crossing_hblank_uses_committed_write_for_entire_row() {
    let rom = word_rom(&[
        0xe3a0_0405, // MOV r0,#0x05000000
        0xe3a0_1e3e, // MOV r1,#0x3e0
        0xe1c0_10b0, // STRH r1,[r0]
    ]);
    let mut m = Memory::new(rom).unwrap();
    m.set_scanline_rendering(true);
    m.write16(PAL, 31).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), m);
    machine.step().unwrap();
    machine.step().unwrap();
    let elapsed = machine.cycles() as u32;
    machine
        .memory_mut()
        .advance_cycles(HBLANK_START - 1 - elapsed);
    machine.step().unwrap();
    assert!(machine.cycles() > u64::from(HBLANK_START));
    let remaining = VBLANK_START - machine.cycles() as u32;
    machine.memory_mut().advance_cycles(remaining);
    // This pins the nominal whole-instruction policy, not pixel-accurate timing.
    assert_eq!(at(&present(machine.memory()), 0, 0), rgb(0x3e0));
}

#[test]
fn failed_cpu_step_does_not_advance_or_discard_capture_progress() {
    let mut m = Memory::new(word_rom(&[0xf000_0000])).unwrap();
    m.set_scanline_rendering(true);
    m.write16(PAL, 31).unwrap();
    m.advance_cycles(HBLANK_START);
    let mut machine = Machine::new(Cpu::new(ROM_START), m);
    let before = machine.memory().display_position();
    let cycles = machine.cycles();
    assert!(machine.step().is_err());
    assert_eq!(machine.cycles(), cycles);
    assert_eq!(machine.memory().display_position(), before);
    machine.memory_mut().write16(PAL, 0x3e0).unwrap();
    machine
        .memory_mut()
        .advance_cycles(VBLANK_START - HBLANK_START);
    let f = present(machine.memory());
    assert_eq!(at(&f, 0, 0), rgb(31));
    assert_eq!(at(&f, 0, 1), rgb(0x3e0));
}

#[test]
fn cpu_raster_demo_copies_table_and_rearms_hblank_dma_across_frames() {
    let mut demo = RasterDemo::new().unwrap();
    let mut f = Framebuffer::default();
    assert_eq!(demo.machine().memory().read16(RASTER_TABLE).unwrap(), 0);
    for update in 1..=40 {
        demo.frame(Buttons::default().with(Button::Right, true), &mut f)
            .unwrap();
        let phase = update % 32;
        let m = demo.machine().memory();
        assert_eq!(m.read32(RASTER_STATE).unwrap(), update);
        assert_eq!(m.read32(RASTER_STATE + 4).unwrap(), phase);
        assert_eq!(m.read16(DMA_BASE + 10).unwrap(), 0xa240);
        assert_eq!(m.captured_vblank(), Some(m.display_position().vblanks));
        assert_eq!(m.display_position().scanline, 160);
        for y in 0..HEIGHT {
            let n = (y as u16 + phase as u16) % 32;
            let color = n + (31 - n) * 32 + (n / 2) * 1024;
            assert!(
                f.pixels()[y * WIDTH..(y + 1) * WIDTH]
                    .iter()
                    .all(|&p| p == rgb(color)),
                "update={update} row={y}"
            );
        }
        let mut snapshot = Framebuffer::default();
        m.render_frame(&mut snapshot).unwrap();
        assert_ne!(snapshot.pixels(), f.pixels()); // Same memory, but no row history in a snapshot.
    }
    assert_eq!(
        demo.machine().memory().read16(RASTER_TABLE).unwrap(),
        31 * 32
    );
}

#[test]
fn raster_controls_wrap_cancel_opposites_and_reset() {
    let mut demo = RasterDemo::new().unwrap();
    let mut f = Framebuffer::default();
    for (buttons, phase) in [
        (Buttons::default().with(Button::Left, true), 31),
        (
            Buttons::default()
                .with(Button::Left, true)
                .with(Button::Right, true),
            31,
        ),
        (Buttons::default().with(Button::Right, true), 0),
        (Buttons::default().with(Button::Right, true), 1),
        (
            Buttons::default()
                .with(Button::Start, true)
                .with(Button::Right, true),
            0,
        ),
    ] {
        demo.frame(buttons, &mut f).unwrap();
        assert_eq!(
            demo.machine().memory().read32(RASTER_STATE + 4).unwrap(),
            phase
        );
        assert_eq!(
            at(&f, 0, 0),
            rgb(phase as u16 | ((31 - phase as u16) << 5) | ((phase as u16 / 2) << 10))
        );
    }
}

fn striped_background(m: &mut Memory) {
    m.write16(PAL, 31).unwrap(); // Backdrop.
    m.write16(PAL + 2, 0x3e0).unwrap();
    for word in 0..8 {
        m.write32(VRAM + word * 4, 0x1111_1111).unwrap(); // Tile 0, all color 1.
    }
    m.write16(0x0400_0008, 0x0100).unwrap(); // BG0: screen block 1, all tile 0.
}

fn background_rows(f: &Framebuffer) -> Vec<usize> {
    (0..HEIGHT)
        .filter(|&y| {
            let row = &f.pixels()[y * WIDTH..(y + 1) * WIDTH];
            assert!(row.iter().all(|&p| p == row[0]), "row {y} is mixed");
            row[0] == rgb(0x3e0)
        })
        .collect()
}

#[test]
fn cpu_background_enable_shows_three_lines_later_and_disable_is_immediate() {
    let rom = word_rom(&[
        0xe3a0_0301, // MOV r0,#0x04000000
        0xe1d0_10b6, // LDRH r1,[r0,#6] (VCOUNT)
        0xe351_0028, // CMP r1,#40
        0x1aff_fffc, // BNE
        0xe1d0_10b4, // LDRH r1,[r0,#4] (DISPSTAT)
        0xe311_0002, // TST r1,#2 (HBlank)
        0x0aff_fffc, // BEQ
        0xe3a0_2c01, // MOV r2,#0x100
        0xe1c0_20b0, // STRH r2,[r0] (BG0 on, in line 40's HBlank)
        0xe1d0_10b6, // LDRH r1,[r0,#6]
        0xe351_0050, // CMP r1,#80
        0x1aff_fffc, // BNE
        0xe1d0_10b4, // LDRH r1,[r0,#4]
        0xe311_0002, // TST r1,#2
        0x0aff_fffc, // BEQ
        0xe3a0_2000, // MOV r2,#0
        0xe1c0_20b0, // STRH r2,[r0] (BG0 off, in line 80's HBlank)
        0xeaff_fffe,
    ]);
    let mut m = Memory::new(rom).unwrap();
    m.set_scanline_rendering(true);
    striped_background(&mut m);
    m.write16(DISPCNT, 0).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), m);
    machine.run_until_vblank(100_000).unwrap();
    // Lines 41 and 42 began before the enable was two lines old.
    assert_eq!(
        background_rows(&present(machine.memory())),
        (43..=80).collect::<Vec<_>>()
    );
}

#[test]
fn host_background_enable_is_visible_at_once() {
    let mut m = memory();
    striped_background(&mut m);
    m.write16(DISPCNT, 0).unwrap();
    m.advance_cycles(CYCLES_PER_LINE * 10 + 100);
    m.write16(DISPCNT, 0x100).unwrap();
    m.advance_cycles(VBLANK_START - CYCLES_PER_LINE * 10 - 100);
    assert_eq!(
        background_rows(&present(&m)),
        (10..HEIGHT).collect::<Vec<_>>()
    );
}

/// Run `poll VCOUNT == line; write DISPCNT = 0x100` and return the rows that show BG0.
fn rows_after_cpu_enable_at(line: u32) -> Vec<usize> {
    let rom = word_rom(&[
        0xe3a0_0301,        // MOV r0,#0x04000000
        0xe1d0_10b6,        // LDRH r1,[r0,#6] (VCOUNT)
        0xe351_0000 | line, // CMP r1,#line
        0x1aff_fffc,        // BNE
        0xe3a0_2c01,        // MOV r2,#0x100
        0xe1c0_20b0,        // STRH r2,[r0]
        0xeaff_fffe,
    ]);
    let mut m = Memory::new(rom).unwrap();
    m.set_scanline_rendering(true);
    striped_background(&mut m);
    m.write16(DISPCNT, 0).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), m);
    machine.run_until_vblank(100_000).unwrap(); // The frame in progress at reset.
    machine.run_until_vblank(100_000).unwrap();
    background_rows(&present(machine.memory()))
}

#[test]
fn background_enabled_during_vblank_shows_from_the_first_row() {
    for line in [160, 200, 226, 227] {
        assert_eq!(
            rows_after_cpu_enable_at(line),
            (0..HEIGHT).collect::<Vec<_>>(),
            "enabled on line {line}"
        );
    }
}

#[test]
fn dma_written_background_enable_waits_like_a_cpu_write() {
    let mut m = Memory::new(word_rom(&[0xeaff_fffe])).unwrap();
    m.set_scanline_rendering(true);
    striped_background(&mut m);
    m.write16(DISPCNT, 0).unwrap();
    m.write16(0x0200_0000, 0x100).unwrap();
    m.write32(DMA_BASE, 0x0200_0000).unwrap();
    m.write32(DMA_BASE + 4, DISPCNT).unwrap();
    m.advance_cycles(CYCLES_PER_LINE * 40 + 100);
    m.write32(DMA_BASE + 8, 0x8000_0001).unwrap(); // Immediate, one halfword.
    let mut machine = Machine::new(Cpu::new(ROM_START), m);
    assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 0 });
    machine.run_until_vblank(100_000).unwrap();
    assert_eq!(
        background_rows(&present(machine.memory())),
        (43..HEIGHT).collect::<Vec<_>>()
    );
}
