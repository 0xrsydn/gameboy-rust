use gba_rust::{
    cpu::Cpu,
    display::{
        CYCLES_PER_FRAME as FRAME, CYCLES_PER_LINE as LINE, HBLANK_START as HB, VBLANK_START as VB,
    },
    dma::DMA_BASE,
    io::*,
    machine::Machine,
    memory::{Memory, OAM_START as OAM, PALETTE_START as PAL, ROM_START, VRAM_START as VRAM},
    video::{rgb555_to_rgb888 as rgb, Framebuffer, HEIGHT, WIDTH},
};

fn color(row: usize) -> u16 {
    ((row % 64 + 1) * 0x101) as u16
}

fn scene() -> Memory {
    let mut m = Memory::new(vec![]).unwrap();
    m.set_scanline_rendering(true);
    m.write16(DISPCNT, 0x1140).unwrap();
    m.write16(BG0CNT, 0x10c0).unwrap(); // 8bpp, mosaic, map block16.
    for index in 0..64 {
        m.write16(PAL + (index + 1) * 2, color(index as usize))
            .unwrap();
        m.write16(PAL + 0x200 + (index + 1) * 2, color(index as usize))
            .unwrap();
    }
    // Eight text tiles, each with a distinct color per row. The map repeats vertically.
    for y in 0..64 {
        for x in (0..8).step_by(2) {
            m.write16(VRAM + y * 8 + x, (y as u16 + 1) * 0x101).unwrap();
        }
    }
    for y in 0..32 {
        for x in 0..32 {
            m.write16(VRAM + 0x8000 + (y * 32 + x) * 2, (y % 8) as u16)
                .unwrap();
        }
    }
    for index in 0..128 {
        m.write16(OAM + index * 8, 0x200).unwrap();
    }
    m.write16(OAM, 0x3000).unwrap(); // Mosaic 8bpp sprite at (32,0), 16x16.
    m.write16(OAM + 2, 0x4020).unwrap();
    for y in 0..16 {
        for x in (0..16).step_by(2) {
            let tile = y / 8 * 2 + x / 8;
            m.write16(
                VRAM + 0x10000 + tile * 64 + y % 8 * 8 + x % 8,
                (y as u16 + 1) * 0x101,
            )
            .unwrap();
        }
    }
    m
}

fn bitmap() -> Memory {
    let mut m = Memory::new(vec![]).unwrap();
    m.set_scanline_rendering(true);
    m.write16(DISPCNT, 0x403).unwrap();
    m.write16(BG2CNT, 0x40).unwrap();
    m.write16(BG2PA, 256).unwrap();
    m.write16(BG2PD, 256).unwrap();
    for y in 0..160 {
        for x in 0..240 {
            m.write16(VRAM + (y * 240 + x) * 2, color(y as usize))
                .unwrap();
        }
    }
    m
}

fn present(m: &Memory) -> Framebuffer {
    let mut f = Framebuffer::default();
    assert!(m.present_frame(&mut f).unwrap());
    f
}

fn finish(m: &mut Memory) -> Framebuffer {
    let p = m.display_position();
    m.advance_cycles(VB - u32::from(p.scanline) * LINE - u32::from(p.line_cycle));
    present(m)
}

fn at(f: &Framebuffer, x: usize, y: usize) -> u32 {
    f.pixels()[y * WIDTH + x]
}

#[test]
fn all_static_heights_match_snapshots_with_independent_background_and_sprite_sizes() {
    for height in 1..=16 {
        let mut m = scene();
        m.write16(MOSAIC, ((height - 1) << 4) | ((16 - height) << 12))
            .unwrap();
        let mut expected = Framebuffer::default();
        m.render_frame(&mut expected).unwrap();
        assert_eq!(
            finish(&mut m).pixels(),
            expected.pixels(),
            "height={height}"
        );
    }
}

#[test]
fn growing_a_block_preserves_counter_instead_of_realigning_to_screen_y() {
    let mut m = scene();
    m.write16(MOSAIC, 0x30).unwrap(); // BG4, OBJ1.
    m.advance_cycles(5 * LINE + HB);
    m.write8(MOSAIC, 0x70).unwrap(); // BG8: phase1 remains phase1.
    let f = finish(&mut m);
    for y in 0..24 {
        let source = if y < 4 {
            0
        } else if y < 12 {
            4
        } else if y < 20 {
            12
        } else {
            20
        };
        assert_eq!(at(&f, 0, y), rgb(color(source)), "row={y}");
        if y < 16 {
            assert_eq!(at(&f, 32, y), rgb(color(y)));
        }
    }
    let mut snapshot = Framebuffer::default();
    m.render_frame(&mut snapshot).unwrap();
    assert_eq!(at(&snapshot, 0, 8), rgb(color(8)));
    assert_eq!(at(&f, 0, 8), rgb(color(4)));
    assert_eq!(present(&m).pixels(), f.pixels());
}

#[test]
fn shrinking_below_current_phase_waits_for_four_bit_wrap() {
    let mut m = scene();
    m.write16(MOSAIC, 0x70).unwrap();
    m.advance_cycles(6 * LINE);
    m.write8(MOSAIC, 0x10).unwrap(); // Phase6 > new height2; no immediate reset.
    m.advance_cycles(0);
    let f = finish(&mut m);
    for y in 0..24 {
        let source = if y < 16 { 0 } else { y / 2 * 2 };
        assert_eq!(at(&f, 0, y), rgb(color(source)), "row={y}");
    }
}

#[test]
fn affine_mosaic_advances_by_current_height_when_counter_wraps_not_by_elapsed_rows() {
    let mut m = bitmap();
    m.write16(MOSAIC, 0x70).unwrap();
    m.advance_cycles(6 * LINE + HB);
    m.write8(MOSAIC, 0x10).unwrap();
    let f = finish(&mut m);
    for y in 0..24 {
        let source = if y < 16 { 0 } else { 2 + (y - 16) / 2 * 2 };
        assert_eq!(at(&f, 0, y), rgb(color(source)), "row={y}");
    }
}

#[test]
fn affine_reference_writes_and_coefficients_do_not_reset_shared_mosaic_phase() {
    let mut m = bitmap();
    m.write16(MOSAIC, 0x70).unwrap();
    m.advance_cycles(5 * LINE);
    m.write32(BG2Y, 20 * 256).unwrap();
    m.write16(BG2PD, 2 * 256).unwrap();
    m.write8(MOSAIC, 0x10).unwrap();
    let f = finish(&mut m);
    for y in 0..20 {
        let source = if y < 5 {
            0
        } else if y < 16 {
            20
        } else {
            24 + (y - 16) / 2 * 4
        };
        assert_eq!(at(&f, 0, y), rgb(color(source)), "row={y}");
    }
}

#[test]
fn mosaic_flags_and_display_enable_do_not_gate_counters() {
    for disabled in [false, true] {
        let mut m = scene();
        m.write16(MOSAIC, 0x7070).unwrap();
        m.write16(BG0CNT, 0x1080).unwrap();
        m.write16(OAM, 0x2000).unwrap();
        if disabled {
            m.write16(DISPCNT, 0).unwrap();
        }
        m.advance_cycles(6 * LINE);
        m.write16(MOSAIC, 0x1010).unwrap();
        m.write16(BG0CNT, 0x10c0).unwrap();
        m.write16(OAM, 0x3000).unwrap();
        m.write16(DISPCNT, 0x1140).unwrap();
        let f = finish(&mut m);
        for y in 6..16 {
            assert_eq!(at(&f, 0, y), rgb(color(0)));
            // Row6 was prepared on row5, before the attribute/enable write.
            let source = if y == 6 && !disabled { 6 } else { 0 };
            assert_eq!(at(&f, 32, y), rgb(color(source)));
        }
    }
}

#[test]
fn forced_blank_does_not_pause_counters() {
    let mut m = scene();
    m.write16(MOSAIC, 0x7070).unwrap();
    m.write16(DISPCNT, 0x11c0).unwrap();
    m.advance_cycles(6 * LINE);
    m.write16(MOSAIC, 0x1010).unwrap();
    m.write16(DISPCNT, 0x1140).unwrap();
    let f = finish(&mut m);
    assert!(f.pixels()[..6 * WIDTH].iter().all(|&p| p == 0xffffff));
    for y in 6..16 {
        assert_eq!(at(&f, 0, y), rgb(color(0)));
    }
}

#[test]
fn sprite_counter_is_independent_and_clamps_partial_first_blocks_before_transform() {
    for affine in [false, true] {
        let mut m = scene();
        m.write16(OAM, if affine { 0x3103 } else { 0x3003 })
            .unwrap(); // y3.
        if affine {
            m.write16(OAM + 6, 256).unwrap();
            m.write16(OAM + 30, 256).unwrap();
        }
        m.write16(MOSAIC, 0x7030).unwrap(); // BG4, OBJ8.
        m.advance_cycles(6 * LINE);
        m.write8(MOSAIC + 1, 0x10).unwrap(); // OBJ2 only.
        let f = finish(&mut m);
        for y in 0..24 {
            assert_eq!(at(&f, 0, y), rgb(color(y / 4 * 4)));
            let source = if !(3..19).contains(&y) {
                y / 4 * 4
            } else if y < 16 {
                0
            } else if y < 18 {
                13
            } else {
                15
            };
            assert_eq!(at(&f, 32, y), rgb(color(source)), "affine={affine} row={y}");
        }
    }
}

#[test]
fn object_window_ignores_live_mosaic_phase() {
    let mut m = scene();
    m.write16(DISPCNT, 0x9140).unwrap();
    m.write16(WINOUT, 0x0100).unwrap(); // Show BG only inside OBJ window.
    m.write16(BG0CNT, 0x1080).unwrap();
    m.write16(OAM, 0x3800).unwrap(); // Mosaic-enabled window sprite.
    for y in (1..16).step_by(2) {
        for x in (0..16).step_by(2) {
            let tile = y / 8 * 2 + x / 8;
            m.write16(VRAM + 0x10000 + tile * 64 + y % 8 * 8 + x % 8, 0)
                .unwrap();
        }
    }
    m.write16(MOSAIC, 0x7000).unwrap();
    m.advance_cycles(6 * LINE);
    m.write16(MOSAIC, 0x1000).unwrap();
    let f = finish(&mut m);
    for y in 0..24 {
        assert_eq!(
            at(&f, 32, y),
            if y < 16 && y % 2 == 0 {
                rgb(color(y))
            } else {
                0
            }
        );
    }
}

#[test]
fn vertical_hold_uses_live_palette_but_sprite_vram_changes_wait_for_preparation() {
    let mut m = scene();
    m.write16(MOSAIC, 0x7070).unwrap();
    m.advance_cycles(3 * LINE + HB);
    m.write16(PAL + 2, 31).unwrap();
    m.write16(VRAM + 0x10000, 0x0202).unwrap();
    let f = finish(&mut m);
    assert_eq!(at(&f, 0, 3), rgb(color(0)));
    assert_eq!(at(&f, 0, 4), rgb(31));
    assert_eq!(at(&f, 32, 3), rgb(color(0)));
    assert_eq!(at(&f, 32, 4), rgb(color(0))); // Prepared before row3 HBlank.
    assert_eq!(at(&f, 32, 5), rgb(color(1)));
}

#[test]
fn hblank_dma_size_writes_preserve_phase_and_affect_next_line_transition() {
    let mut m = scene();
    m.write16(MOSAIC, 0x7070).unwrap();
    for y in 0..160 {
        m.write16(0x0200_0000 + y * 2, if y < 6 { 0x7070 } else { 0x3010 })
            .unwrap();
    }
    m.write32(DMA_BASE, 0x0200_0000).unwrap();
    m.write32(DMA_BASE + 4, MOSAIC).unwrap();
    m.write32(DMA_BASE + 8, 0xa240_0001).unwrap();
    m.write8(HALTCNT, 0).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), m);
    machine.run_until_vblank(1000).unwrap();
    let f = present(machine.memory());
    // Independent recurrence, driven by the table value written after each row.
    let (mut bg, mut obj) = (0usize, 0usize);
    for y in 0..HEIGHT {
        assert_eq!(at(&f, 0, y), rgb(color(y - bg)), "row={y}");
        if y < 16 {
            assert_eq!(at(&f, 32, y), rgb(color(y - obj)));
        }
        let (bg_height, obj_height) = if y < 6 { (8, 8) } else { (2, 4) };
        bg = if bg + 1 == bg_height {
            0
        } else {
            (bg + 1) % 16
        };
        obj = if obj + 1 == obj_height {
            0
        } else {
            (obj + 1) % 16
        };
    }
    assert!(machine.halted());
}

#[test]
fn vblank_resets_counters_and_capture_toggles_do_not_reset_them() {
    let mut m = scene();
    m.set_scanline_rendering(false);
    m.write16(MOSAIC, 0x7070).unwrap();
    m.advance_cycles(6 * LINE);
    m.write16(MOSAIC, 0x1010).unwrap();
    m.advance_cycles(VB - 6 * LINE);
    m.write16(MOSAIC, 0x5050).unwrap(); // New size during VBlank.
    m.advance_cycles(FRAME - VB);
    m.set_scanline_rendering(true);
    let f = finish(&mut m);
    for y in 0..16 {
        assert_eq!(at(&f, 0, y), rgb(color(y / 6 * 6)));
        assert_eq!(at(&f, 32, y), rgb(color(y / 6 * 6)));
    }
}
