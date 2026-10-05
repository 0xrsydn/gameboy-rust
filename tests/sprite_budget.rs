//! Regression tests for the nominal allowance, not hardware fetch cutoffs.
use gba_rust::{
    cpu::Cpu,
    display::{CYCLES_PER_LINE as LINE, VBLANK_START as VB},
    dma::DMA_BASE,
    io::*,
    machine::Machine,
    memory::{Memory, OAM_START as OAM, PALETTE_START as PAL, ROM_START, VRAM_START as VRAM},
    video::{rgb555_to_rgb888 as rgb, Framebuffer, VideoError, WIDTH},
};

fn scene(free: bool) -> Memory {
    let mut m = Memory::new(vec![]).unwrap();
    m.set_scanline_rendering(true);
    m.write16(DISPCNT, if free { 0x1060 } else { 0x1040 })
        .unwrap();
    m.write16(PAL, 0x7c00).unwrap();
    m.write16(PAL + 0x202, 31).unwrap();
    m.write16(PAL + 0x204, 0x3e0).unwrap();
    for i in 0..128 {
        obj(&mut m, i, 0x200, 0, 0);
    }
    // Tile zero is transparent. Separate solid 64x64 textures use indices 1/2.
    for offset in (0..2048).step_by(2) {
        m.write16(VRAM + 0x11000 + offset, 0x1111).unwrap();
        m.write16(VRAM + 0x12000 + offset, 0x2222).unwrap();
        m.write16(VRAM + 0x14000 + offset, 0x2222).unwrap();
    }
    m
}

fn obj(m: &mut Memory, index: u32, a: u16, b: u16, c: u16) {
    m.write16(OAM + index * 8, a).unwrap();
    m.write16(OAM + index * 8 + 2, b).unwrap();
    m.write16(OAM + index * 8 + 4, c).unwrap();
}

fn fillers(m: &mut Memory, count: u32) {
    for i in 0..count {
        obj(m, i, 0, 0xc000, 0);
    }
}

fn snapshot(m: &Memory) -> Framebuffer {
    let before = (m.cycles(), m.display_position());
    let mut f = Framebuffer::default();
    m.render_frame(&mut f).unwrap();
    assert_eq!((m.cycles(), m.display_position()), before);
    f
}

fn finish(m: &mut Memory) -> Framebuffer {
    let p = m.display_position();
    m.advance_cycles(VB - u32::from(p.scanline) * LINE - u32::from(p.line_cycle));
    let mut f = Framebuffer::default();
    assert!(m.present_frame(&mut f).unwrap());
    f
}

fn at(f: &Framebuffer, x: usize, y: usize) -> u32 {
    f.pixels()[y * WIDTH + x]
}

fn prefix(f: &Framebuffer, y: usize, left: usize, width: usize, visible: usize) {
    for x in left..left + width {
        assert_eq!(
            at(f, x, y),
            rgb(if x < left + visible { 0x3e0 } else { 0x7c00 }),
            "pixel={x},{y}, prefix={visible}"
        );
    }
}

#[test]
fn regular_prefix_exhausts_allowance_and_omits_later_entries() {
    for free in [false, true] {
        let mut m = scene(free);
        let count = if free { 14 } else { 18 };
        fillers(&mut m, count);
        obj(&mut m, count, 0, 0xc000 | 80, 256 | 0xc00);
        obj(&mut m, count + 1, 0, 0xc000 | 160, 256);
        let f = snapshot(&m);
        prefix(&f, 0, 80, 64, 58);
        prefix(&f, 0, 160, 64, 0);
        assert_eq!(finish(&mut m).pixels(), f.pixels());
    }
}

#[test]
fn small_sprites_have_no_fixed_thirty_two_object_limit() {
    for (free, count, visible) in [(false, 127, 8), (true, 119, 2), (true, 120, 0)] {
        let mut m = scene(free);
        for i in 0..count {
            obj(&mut m, i, 0, 0, 0);
        }
        obj(&mut m, count, 0, 80, 256);
        prefix(&snapshot(&m), 0, 80, 8, visible);
    }
}

#[test]
fn affine_setup_and_double_size_use_drawing_canvas_width() {
    for free in [false, true] {
        for doubled in [false, true] {
            let mut m = scene(free);
            let count = if free { 14 } else { 18 };
            fillers(&mut m, count); // 58 cycles remain.
                                    // Zero matrices sample the opaque center for the entire canvas.
            obj(&mut m, count, 0x100, 80, 256); // 10+2*8 = 26.
            obj(
                &mut m,
                count + 1,
                if doubled { 0x300 } else { 0x100 },
                if doubled { 112 } else { 0x4000 | 112 },
                256,
            ); // 32 cycles remain: setup plus 11 canvas pixels.
            let f = snapshot(&m);
            prefix(&f, 0, 80, 8, 8);
            prefix(&f, 0, 112, 16, 11);
        }
    }
}

#[test]
fn unfinished_affine_setup_does_not_refund_work_to_regular_objects() {
    for free in [false, true] {
        let mut m = scene(free);
        let count = if free { 14 } else { 18 };
        fillers(&mut m, count);
        obj(&mut m, count, 0, 0x8000, 0); // 32 cycles.
        obj(&mut m, count + 1, 0, 0x4000, 0); // 16 cycles.
        obj(&mut m, count + 2, 0x100, 80, 256); // Only setup fits.
        obj(&mut m, count + 3, 0, 96, 256);
        let f = snapshot(&m);
        prefix(&f, 0, 80, 8, 0);
        prefix(&f, 0, 96, 8, 0);
    }
}

#[test]
fn disabled_off_row_and_fully_offscreen_entries_cost_inspection_only() {
    for free in [false, true] {
        for (a, b) in [(0x200, 0), (80, 0xc000), (0, 0xc000 | 240)] {
            let mut m = scene(free);
            for i in 0..100 {
                obj(&mut m, i, a, b, 0);
            }
            let count = if free { 11 } else { 15 };
            for i in 100..100 + count {
                obj(&mut m, i, 0, 0xc000, 0);
            }
            obj(&mut m, 100 + count, 0, 0xc000 | 80, 256);
            prefix(&snapshot(&m), 0, 80, 64, 50);
        }
    }
}

#[test]
fn partially_clipped_objects_charge_full_canvas_in_this_nominal_model() {
    let mut m = scene(false);
    fillers(&mut m, 18);
    obj(&mut m, 18, 0, 0xc000 | 508, 256); // Left four pixels are clipped.
    prefix(&snapshot(&m), 0, 0, 60, 54);

    let mut m = scene(false);
    fillers(&mut m, 17);
    obj(&mut m, 17, 0, 0xc000 | 238, 256); // Only two pixels are on screen.
    obj(&mut m, 18, 0, 0xc000 | 80, 256);
    let f = snapshot(&m);
    prefix(&f, 0, 238, 2, 2);
    prefix(&f, 0, 80, 64, 58);
}

#[test]
fn transparent_semitransparent_and_disabled_obj_window_pixels_still_cost_work() {
    for a in [0, 0x400, 0x800] {
        let mut m = scene(false);
        for i in 0..18 {
            obj(&mut m, i, a, 0xc000, if a == 0x800 { 128 } else { 0 });
        }
        obj(&mut m, 18, 0, 0xc000 | 80, 256);
        prefix(&snapshot(&m), 0, 80, 64, 58);
    }
}

#[test]
fn oam_order_not_display_priority_allocates_work_even_for_occluded_objects() {
    let mut m = scene(false);
    for i in 0..18 {
        obj(&mut m, i, 0, 0xc000 | 80, 128 | 0xc00);
    }
    obj(&mut m, 18, 0, 0xc000 | 80, 256);
    let f = snapshot(&m);
    for x in 80..144 {
        assert_eq!(at(&f, x, 0), rgb(if x < 138 { 0x3e0 } else { 31 }));
    }
}

#[test]
fn large_affine_and_out_of_texture_samples_use_full_canvas_cost() {
    for doubled in [false, true] {
        let mut m = scene(false);
        let count = if doubled { 4 } else { 8 };
        for i in 0..count {
            obj(&mut m, i, if doubled { 0x300 } else { 0x100 }, 0xc000, 0);
        }
        // Most samples are outside the texture. This does not refund work.
        m.write16(OAM + 6, 0x7fff).unwrap();
        m.write16(OAM + 30, 0x7fff).unwrap();
        obj(&mut m, count, 0, 0xc000 | 16, 256);
        obj(&mut m, count + 1, 0, 0xc000 | 80, 256);
        obj(&mut m, count + 2, 0, 0xc000 | 144, 256);
        let f = snapshot(&m);
        prefix(&f, 0, 16, 64, 64);
        prefix(&f, 0, 80, 64, if doubled { 64 } else { 42 });
        prefix(&f, 0, 144, 64, if doubled { 18 } else { 0 });
    }
}

#[test]
fn bitmap_restricted_tiles_do_not_refund_sprite_work() {
    let mut m = scene(false);
    m.write16(DISPCNT, 0x1043).unwrap();
    fillers(&mut m, 18); // Tile zero is unavailable to OBJ in bitmap modes.
    obj(&mut m, 18, 0, 0xc000 | 80, 512);
    prefix(&snapshot(&m), 0, 80, 64, 58);
}

#[test]
fn each_row_gets_an_independent_allowance() {
    let mut m = scene(false);
    for i in 0..18 {
        obj(&mut m, i, 1, 0xc000, 0);
    }
    obj(&mut m, 18, 0, 0xc000 | 80, 256);
    let f = snapshot(&m);
    prefix(&f, 0, 80, 64, 64);
    prefix(&f, 1, 80, 64, 58);
    assert_eq!(finish(&mut m).pixels(), f.pixels());
}

#[test]
fn hblank_free_is_sampled_at_preparation_not_composition() {
    for elapsed in [39, 40, 41] {
        let mut m = scene(false);
        fillers(&mut m, 18);
        obj(&mut m, 18, 0, 0xc000 | 80, 256);
        m.advance_cycles(elapsed);
        m.write16(DISPCNT, 0x1060).unwrap();
        prefix(&snapshot(&m), 0, 80, 64, 0);
        let f = finish(&mut m);
        prefix(&f, 0, 80, 64, 58);
        prefix(&f, 1, 80, 64, if elapsed < 40 { 0 } else { 58 });
        prefix(&f, 2, 80, 64, 0);
    }
}

#[test]
fn hblank_dma_updates_allowance_for_the_following_preparation() {
    let mut m = scene(false);
    fillers(&mut m, 18);
    obj(&mut m, 18, 0, 0xc000 | 80, 256);
    for row in 0..160 {
        m.write16(
            0x0200_0000 + row * 2,
            if row % 2 == 0 { 0x1060 } else { 0x1040 },
        )
        .unwrap();
    }
    m.write32(DMA_BASE, 0x0200_0000).unwrap();
    m.write32(DMA_BASE + 4, DISPCNT).unwrap();
    m.write32(DMA_BASE + 8, 0xa240_0001).unwrap();
    m.write8(HALTCNT, 0).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), m);
    machine.run_until_vblank(1000).unwrap();
    let mut f = Framebuffer::default();
    assert!(machine.memory().present_frame(&mut f).unwrap());
    for y in 0..64 {
        prefix(&f, y, 80, 64, if y < 2 || y % 2 == 1 { 58 } else { 0 });
    }
    assert!(machine.halted());
}

#[test]
fn exhausted_work_preserves_development_diagnostics_and_output_atomicity() {
    let mut m = scene(false);
    fillers(&mut m, 19);
    obj(&mut m, 127, 0xc00, 0, 0);
    let mut f = Framebuffer::default();
    f.clear(0x3e0);
    assert!(matches!(
        m.render_frame(&mut f),
        Err(VideoError::UnsupportedObject { index: 127, .. })
    ));
    assert!(f.pixels().iter().all(|&p| p == rgb(0x3e0)));
    m.advance_cycles(VB);
    assert!(matches!(
        m.present_frame(&mut f),
        Err(VideoError::UnsupportedObject { index: 127, .. })
    ));
    assert!(f.pixels().iter().all(|&p| p == rgb(0x3e0)));
    for control in [0x40, 0x10c0] {
        m.write16(DISPCNT, control).unwrap();
        m.render_frame(&mut f).unwrap();
    }
}
