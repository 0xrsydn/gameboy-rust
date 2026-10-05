use gba_core::{
    cpu::Cpu,
    display::{CYCLES_PER_FRAME, CYCLES_PER_LINE as LINE, HBLANK_START as HB, VBLANK_START as VB},
    dma::DMA_BASE,
    input::{Button, Buttons},
    io::*,
    machine::Machine,
    memory::{Memory, PALETTE_START as PAL, ROM_START, VRAM_START as VRAM},
    video::{rgb555_to_rgb888 as rgb, Framebuffer, HEIGHT, WIDTH},
};
use gba_demos::affine_raster_demo::{AffineRasterDemo, AFFINE_RASTER_STATE, AFFINE_RASTER_TABLE};

fn color(x: u32, y: u32) -> u16 {
    ((x & 31) | ((y & 31) << 5) | (((x + y) & 31) << 10)) as u16
}

fn bitmap() -> Memory {
    let mut m = Memory::new(vec![]).unwrap();
    m.set_scanline_rendering(true);
    m.write16(DISPCNT, 0x403).unwrap();
    m.write16(BG2PA, 256).unwrap();
    m.write16(BG2PB, 256).unwrap();
    m.write16(BG2PD, 256).unwrap();
    for y in 0..160 {
        for x in 0..240 {
            m.write16(VRAM + (y * 240 + x) * 2, color(x, y)).unwrap();
        }
    }
    m
}

fn finish(m: &mut Memory) -> Framebuffer {
    let p = m.display_position();
    let phase = u32::from(p.scanline) * LINE + u32::from(p.line_cycle);
    m.advance_cycles(VB - phase);
    let mut f = Framebuffer::default();
    assert!(m.present_frame(&mut f).unwrap());
    f
}

fn row(f: &Framebuffer, y: usize, sx: u32, sy: u32) {
    assert_eq!(f.pixels()[y * WIDTH], rgb(color(sx, sy)), "row={y}");
}

#[test]
fn coefficient_changes_accumulate_instead_of_multiplying_absolute_screen_y() {
    let mut m = bitmap();
    m.advance_cycles(2 * LINE + HB);
    m.write16(BG2PB, 3 * 256).unwrap();
    m.write16(BG2PD, 2 * 256).unwrap();
    let f = finish(&mut m);
    for (y, x, sy) in [(0, 0, 0), (1, 1, 1), (2, 2, 2), (3, 5, 4), (4, 8, 6)] {
        row(&f, y, x, sy);
    }
    let mut snapshot = Framebuffer::default();
    m.render_frame(&mut snapshot).unwrap();
    row(&snapshot, 3, 9, 6); // Debug snapshots still use programmed origins.
}

#[test]
fn visible_reference_writes_replace_current_origin_and_only_the_written_axis() {
    let mut m = bitmap();
    m.advance_cycles(3 * LINE);
    m.write32(BG2X, 10 * 256).unwrap();
    m.advance_cycles(HB);
    m.write32(BG2Y, 20 * 256).unwrap(); // HBlank: override next Y increment.
    let f = finish(&mut m);
    row(&f, 2, 2, 2);
    row(&f, 3, 10, 3);
    row(&f, 4, 11, 20);
    row(&f, 5, 12, 21);
}

#[test]
fn partial_reference_writes_merge_programmed_not_accumulated_values() {
    let mut m = bitmap();
    m.write32(BG2X, 0x200).unwrap();
    m.advance_cycles(5 * LINE);
    m.write8(BG2X, 0x80).unwrap();
    let f = finish(&mut m);
    row(&f, 4, 6, 4);
    row(&f, 5, 2, 5);
    row(&f, 6, 3, 6);
    assert_eq!(m.read32(BG2X).unwrap(), 0); // Still write-only.
}

#[test]
fn line_zero_reloads_programmed_origins_after_vblank_writes() {
    let mut m = bitmap();
    finish(&mut m);
    m.write32(BG2X, 7 * 256).unwrap();
    m.write32(BG2Y, 9 * 256).unwrap();
    m.advance_cycles(CYCLES_PER_FRAME - VB);
    let f = finish(&mut m);
    row(&f, 0, 7, 9);
    row(&f, 1, 8, 10);
    m.advance_cycles(CYCLES_PER_FRAME - VB);
    let f = finish(&mut m);
    row(&f, 0, 7, 9); // Not the last frame's accumulated position.
}

#[test]
fn disabled_and_non_affine_modes_pause_tracking_but_forced_blank_does_not() {
    for disabled in [3, 0x400, 0x483] {
        let mut m = bitmap();
        m.advance_cycles(2 * LINE);
        m.write16(DISPCNT, disabled).unwrap();
        m.advance_cycles(3 * LINE);
        m.write16(DISPCNT, 0x403).unwrap();
        let f = finish(&mut m);
        let origin = if disabled == 0x483 { 5 } else { 2 };
        row(&f, 5, origin, origin);
        if disabled == 0x483 {
            assert_eq!(f.pixels()[2 * WIDTH], 0xffffff);
        }
    }
}

#[test]
fn window_masks_do_not_pause_internal_tracking() {
    let mut m = bitmap();
    m.write16(DISPCNT, 0x2403).unwrap();
    m.write16(WIN0H, 240).unwrap();
    m.write16(WIN0V, 160).unwrap();
    m.write16(WININ, 0).unwrap();
    m.advance_cycles(5 * LINE);
    m.write16(WININ, 4).unwrap();
    let f = finish(&mut m);
    assert_eq!(f.pixels()[4 * WIDTH], 0);
    row(&f, 5, 5, 5);
}

#[test]
fn mosaic_holds_origin_then_uses_current_coefficient_for_block_increment() {
    let mut m = bitmap();
    m.write16(BG2CNT, 0x40).unwrap();
    m.write16(MOSAIC, 0x30).unwrap(); // Four-row vertical blocks.
    m.advance_cycles(2 * LINE + HB);
    m.write16(BG2PB, 2 * 256).unwrap();
    let f = finish(&mut m);
    for y in 0..4 {
        row(&f, y, 0, 0);
    }
    for y in 4..8 {
        row(&f, y, 8, 4);
    }
    row(&f, 8, 16, 8);
}

#[test]
fn bg2_and_bg3_track_independently_in_supported_tiled_modes() {
    for mode in [1, 2] {
        let mut m = Memory::new(vec![]).unwrap();
        m.set_scanline_rendering(true);
        m.write16(DISPCNT, 0x400 | mode).unwrap();
        m.write16(BG2CNT, 0x1000).unwrap();
        m.write16(BG3CNT, 0x1000).unwrap();
        for reg in [BG2PA, BG3PA] {
            m.write16(reg, 256).unwrap();
        }
        m.write16(BG2PB, 256).unwrap();
        m.write16(BG3PB, 2 * 256).unwrap();
        for index in 1..=8 {
            m.write16(PAL + index * 2, color(index, 0)).unwrap();
        }
        for y in 0..8 {
            for x in (0..8).step_by(2) {
                m.write16(VRAM + y * 8 + x, (x + 1) as u16 | (((x + 2) as u16) << 8))
                    .unwrap();
            }
        }
        m.advance_cycles(3 * LINE);
        m.write16(DISPCNT, if mode == 2 { 0x802 } else { 0x401 })
            .unwrap();
        let f = finish(&mut m);
        row(&f, 2, 3, 0);
        if mode == 2 {
            row(&f, 3, 1, 0); // Disabled BG3 did not accumulate.
            row(&f, 4, 3, 0);
            row(&f, 5, 5, 0);
        } else {
            row(&f, 4, 5, 0); // BG2 remains affine in mode1.
        }
    }
}

#[test]
fn capture_enable_does_not_reset_affine_hardware_state() {
    let mut m = bitmap();
    m.set_scanline_rendering(false);
    m.advance_cycles(CYCLES_PER_FRAME); // Batched clock path.
    m.set_scanline_rendering(true);
    m.advance_cycles(4 * LINE);
    m.write32(BG2X, 9 * 256).unwrap();
    let captured = finish(&mut m);
    row(&captured, 4, 9, 4);
    m.set_scanline_rendering(false);
    m.advance_cycles(CYCLES_PER_FRAME - VB);
    m.set_scanline_rendering(true);
    let captured = finish(&mut m);
    row(&captured, 0, 9, 0);
}

#[test]
fn hblank_dma_reference_writes_override_next_increment() {
    let mut m = bitmap();
    for y in 0..160 {
        m.write32(0x0200_0000 + y * 4, (y % 16 + 10) * 256).unwrap();
    }
    m.write32(DMA_BASE, 0x0200_0000).unwrap();
    m.write32(DMA_BASE + 4, BG2X).unwrap();
    m.write32(DMA_BASE + 8, 0xa640_0001).unwrap(); // Word, fixed destination, repeat HBlank.
    m.write8(HALTCNT, 0).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), m);
    machine.run_until_vblank(1000).unwrap();
    let mut f = Framebuffer::default();
    assert!(machine.memory().present_frame(&mut f).unwrap());
    row(&f, 0, 0, 0);
    for y in 1..HEIGHT {
        row(&f, y, (y as u32 - 1) % 16 + 10, y as u32);
    }
}

#[test]
fn cpu_raster_demo_generates_dma_table_and_all_transform_combinations() {
    let mut demo = AffineRasterDemo::new().unwrap();
    let mut f = Framebuffer::default();
    for index in 0..8 {
        let rotated = index & 1 != 0;
        let zoomed = index & 2 != 0;
        let bypassed = index & 4 != 0;
        let buttons = Buttons::default()
            .with(Button::L, rotated)
            .with(Button::R, zoomed)
            .with(Button::A, bypassed)
            .with(Button::Left, true)
            .with(Button::Up, true);
        demo.frame(buttons, &mut f).unwrap();
        let m = demo.machine().memory();
        let pan = -2 * (index + 1);
        assert_eq!(m.read32(AFFINE_RASTER_STATE).unwrap(), index as u32 + 1);
        assert_eq!(m.read32(AFFINE_RASTER_STATE + 4).unwrap() as i32, pan);
        assert_eq!(m.read32(AFFINE_RASTER_STATE + 8).unwrap() as i32, pan);
        assert_eq!(m.read16(DMA_BASE + 10).unwrap(), 0xa240);
        assert_eq!(m.captured_vblank(), Some(m.display_position().vblanks));
        let a = match (rotated, zoomed) {
            (false, false) => 256,
            (false, true) => 128,
            (true, false) => 181,
            (true, true) => 90,
        };
        let b = if rotated { a } else { 0 };
        for y in 0..160 {
            let offset = if bypassed {
                0
            } else if y & 16 == 0 {
                512
            } else {
                -512
            };
            assert_eq!(
                m.read16(AFFINE_RASTER_TABLE + y * 2).unwrap() as i16,
                (b + offset) as i16
            );
        }
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let dx = x as i32 - 120;
                let dy = y as i32 - 80;
                let phase = y as i32 % 32;
                let extra = if bypassed {
                    0
                } else {
                    512 * phase.min(32 - phase)
                };
                let tx = ((128 + pan) * 256 + a * dx + b * dy + extra)
                    .div_euclid(256)
                    .rem_euclid(256) as usize;
                let ty = ((128 + pan) * 256 - b * dx + a * dy)
                    .div_euclid(256)
                    .rem_euclid(256) as usize;
                let tile = (tx / 32 + 2 * (ty / 32)) % 4;
                let shade = (tx / 2 + ty / 2) % 2;
                let expected = [
                    0x0260, 0x03a0, 0x7d20, 0x7e80, 0x001f, 0x421f, 0x03ff, 0x7fff,
                ][tile * 2 + shade];
                assert_eq!(
                    f.pixels()[y * WIDTH + x],
                    rgb(expected),
                    "case={index} x={x} y={y}"
                );
            }
        }
    }
    demo.frame(
        Buttons::default()
            .with(Button::Left, true)
            .with(Button::Right, true),
        &mut f,
    )
    .unwrap();
    assert_eq!(
        demo.machine()
            .memory()
            .read32(AFFINE_RASTER_STATE + 4)
            .unwrap() as i32,
        -16
    );
    demo.frame(
        Buttons::default()
            .with(Button::Start, true)
            .with(Button::Right, true),
        &mut f,
    )
    .unwrap();
    assert_eq!(
        demo.machine()
            .memory()
            .read32(AFFINE_RASTER_STATE + 4)
            .unwrap(),
        0
    );
    assert_eq!(
        demo.machine()
            .memory()
            .read32(AFFINE_RASTER_STATE + 8)
            .unwrap(),
        0
    );
}
