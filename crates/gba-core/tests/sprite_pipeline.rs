use gba_core::{
    cpu::Cpu,
    display::{CYCLES_PER_FRAME as FRAME, CYCLES_PER_LINE as LINE, VBLANK_START as VB},
    dma::DMA_BASE,
    io::*,
    machine::Machine,
    memory::{Memory, OAM_START as OAM, PALETTE_START as PAL, ROM_START, VRAM_START as VRAM},
    video::{rgb555_to_rgb888 as rgb, Framebuffer, VideoError, WIDTH},
};

fn scene() -> Memory {
    let mut m = Memory::new(vec![]).unwrap();
    m.set_scanline_rendering(true);
    m.write16(DISPCNT, 0x1040).unwrap();
    m.write16(PAL, 0x7c00).unwrap();
    m.write16(PAL + 0x202, 31).unwrap();
    m.write16(PAL + 0x204, 0x3e0).unwrap();
    for i in 0..128 {
        m.write16(OAM + i * 8, 0x200).unwrap();
    }
    m.write16(OAM, 0).unwrap();
    for offset in (0..32).step_by(2) {
        m.write16(VRAM + 0x10000 + offset, 0x1111).unwrap();
    }
    m
}

fn finish(m: &mut Memory) -> Framebuffer {
    let p = m.display_position();
    m.advance_cycles(VB - u32::from(p.scanline) * LINE - u32::from(p.line_cycle));
    present(m)
}

fn present(m: &Memory) -> Framebuffer {
    let mut f = Framebuffer::default();
    assert!(m.present_frame(&mut f).unwrap());
    f
}

fn at(f: &Framebuffer, x: usize, y: usize) -> u32 {
    f.pixels()[y * WIDTH + x]
}

#[test]
fn oam_position_writes_obey_preceding_lines_cycle40_boundary() {
    for elapsed in [39, 40, 41] {
        let mut m = scene();
        m.advance_cycles(elapsed);
        m.write16(OAM + 2, 16).unwrap();
        let f = finish(&mut m);
        assert_eq!(at(&f, 0, 0), rgb(31)); // Reset row0 was already prepared.
        let x = if elapsed < 40 { 16 } else { 0 };
        assert_eq!(at(&f, x, 1), rgb(31));
        assert_eq!(at(&f, 16 - x, 1), rgb(0x7c00));
        assert_eq!(at(&f, 16, 2), rgb(31));
    }
}

#[test]
fn buffered_indices_keep_vram_history_but_resolve_current_palette() {
    let mut m = scene();
    m.advance_cycles(40);
    for offset in (0..32).step_by(2) {
        m.write16(VRAM + 0x10000 + offset, 0x2222).unwrap();
    }
    m.write16(PAL + 0x202, 0x7fff).unwrap();
    let f = finish(&mut m);
    assert_eq!(at(&f, 0, 0), rgb(0x7fff));
    assert_eq!(at(&f, 0, 1), rgb(0x7fff));
    assert_eq!(at(&f, 0, 2), rgb(0x3e0));
    let mut snapshot = Framebuffer::default();
    m.render_frame(&mut snapshot).unwrap();
    assert_eq!(at(&snapshot, 0, 1), rgb(0x3e0));
    assert_eq!(present(&m).pixels(), f.pixels());
}

#[test]
fn affine_matrix_and_tile_samples_are_frozen_at_preparation() {
    let mut m = scene();
    m.write16(OAM, 0x2100).unwrap(); // Affine, 8bpp.
    m.write16(OAM + 6, 256).unwrap();
    m.write16(OAM + 30, 256).unwrap();
    for y in 0..8 {
        for x in (0..8).step_by(2) {
            m.write16(
                VRAM + 0x10000 + y * 8 + x,
                if x < 4 { 0x0101 } else { 0x0202 },
            )
            .unwrap();
        }
    }
    m.advance_cycles(40);
    m.write16(OAM + 6, 0).unwrap(); // PA0 samples the texture center for every X.
    let f = finish(&mut m);
    assert_eq!(at(&f, 0, 1), rgb(31));
    assert_eq!(at(&f, 0, 2), rgb(0x3e0));
}

#[test]
fn semitransparency_is_buffered_but_blend_coefficients_remain_live() {
    let mut m = scene();
    m.write16(OAM, 0x400).unwrap();
    m.advance_cycles(40);
    m.write16(OAM, 0).unwrap();
    m.write16(BLDCNT, 0x2000).unwrap(); // Backdrop as second target.
    m.write16(BLDALPHA, 0x0808).unwrap();
    let f = finish(&mut m);
    assert_eq!(at(&f, 0, 1), rgb(0x3c0f));
    assert_eq!(at(&f, 0, 2), rgb(31));
}

#[test]
fn obj_window_coverage_is_buffered_even_before_its_display_mask_is_enabled() {
    let mut m = scene();
    m.write16(OAM, 0x800).unwrap();
    m.advance_cycles(40);
    m.write16(OAM + 2, 16).unwrap();
    m.write16(DISPCNT, 0x9040).unwrap();
    m.write16(WINOUT, 0x2000).unwrap();
    m.write16(BLDCNT, 0xa0).unwrap();
    m.write16(BLDY, 16).unwrap();
    let f = finish(&mut m);
    assert_eq!(at(&f, 0, 1), rgb(0x7fff));
    assert_eq!(at(&f, 16, 1), rgb(0x7c00));
    assert_eq!(at(&f, 0, 2), rgb(0x7c00));
    assert_eq!(at(&f, 16, 2), rgb(0x7fff));
}

#[test]
fn horizontal_mosaic_uses_current_size_and_prepared_sprite_metadata() {
    let mut m = scene();
    m.write16(OAM, 0x1000).unwrap();
    for offset in (0..32).step_by(2) {
        m.write16(VRAM + 0x10000 + offset, 0x2121).unwrap();
    }
    m.advance_cycles(40);
    m.write16(OAM, 0).unwrap();
    m.write16(MOSAIC, 0x0300).unwrap();
    let f = finish(&mut m);
    assert_eq!(at(&f, 1, 1), rgb(31));
    assert_eq!(at(&f, 1, 2), rgb(0x3e0));
}

#[test]
fn global_enable_gates_preparation_and_current_composition() {
    let mut m = scene();
    m.write16(DISPCNT, 0x40).unwrap();
    m.advance_cycles(40);
    m.write16(DISPCNT, 0x1040).unwrap();
    let f = finish(&mut m);
    assert_eq!(at(&f, 0, 0), rgb(0x7c00));
    assert_eq!(at(&f, 0, 1), rgb(0x7c00));
    assert_eq!(at(&f, 0, 2), rgb(31));

    let mut m = scene();
    m.advance_cycles(40);
    m.write16(DISPCNT, 0x40).unwrap();
    assert_eq!(at(&finish(&mut m), 0, 1), rgb(0x7c00));
}

#[test]
fn forced_blank_does_not_stop_preparation_in_this_row_model() {
    let mut m = scene();
    m.write16(DISPCNT, 0x10c0).unwrap();
    m.advance_cycles(40);
    m.write16(OAM + 2, 16).unwrap();
    m.write16(DISPCNT, 0x1040).unwrap();
    let f = finish(&mut m);
    assert_eq!(at(&f, 0, 1), rgb(31));
    assert_eq!(at(&f, 16, 2), rgb(31));
}

#[test]
fn hblank_dma_oam_changes_wait_until_a_later_preparation_event() {
    let mut m = scene();
    for row in 0..160 {
        m.write16(0x0200_0000 + row * 2, ((row + 1) % 20 * 8) as u16)
            .unwrap();
    }
    m.write32(DMA_BASE, 0x0200_0000).unwrap();
    m.write32(DMA_BASE + 4, OAM + 2).unwrap();
    m.write32(DMA_BASE + 8, 0xa240_0001).unwrap();
    m.write8(HALTCNT, 0).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), m);
    machine.run_until_vblank(1000).unwrap();
    let f = present(machine.memory());
    for y in 0usize..8 {
        let left = y.saturating_sub(1) * 8;
        for x in 0..WIDTH {
            assert_eq!(
                at(&f, x, y),
                rgb(if (left..left + 8).contains(&x) {
                    31
                } else {
                    0x7c00
                }),
                "pixel={x},{y}"
            );
        }
    }
    assert!(machine.halted());
}

#[test]
fn final_hidden_line_prepares_row_zero_before_frame_wrap() {
    for cycle in [39, 40] {
        let mut m = scene();
        finish(&mut m);
        m.advance_cycles(227 * LINE + cycle - VB);
        m.write16(OAM + 2, 16).unwrap();
        m.advance_cycles(FRAME - 227 * LINE - cycle);
        let f = finish(&mut m);
        assert_eq!(at(&f, if cycle < 40 { 16 } else { 0 }, 0), rgb(31));
        assert_eq!(at(&f, 16, 1), rgb(31));
    }
}

#[test]
fn capture_toggles_preserve_prepared_rows_after_large_capture_disabled_batches() {
    for frames in [0, 3] {
        let mut m = scene();
        m.set_scanline_rendering(false);
        m.advance_cycles(frames * FRAME + 41);
        m.write16(OAM + 2, 16).unwrap();
        m.set_scanline_rendering(true);
        m.set_scanline_rendering(false);
        m.set_scanline_rendering(true);
        let f = finish(&mut m);
        assert_eq!(at(&f, 0, 0), rgb(31));
        assert_eq!(at(&f, 0, 1), rgb(31));
        assert_eq!(at(&f, 16, 2), rgb(31));
    }
}

#[test]
fn prepared_errors_are_deferred_until_visible_composition_and_recover_next_frame() {
    let mut m = scene();
    m.advance_cycles(1); // Bootstrap row0 remains valid.
    m.write16(OAM, 0xc00).unwrap();
    m.advance_cycles(39); // Prepare invalid row1; clock advancement itself succeeds.
    m.write16(OAM, 0).unwrap();
    m.advance_cycles(VB - 40);
    let mut f = Framebuffer::default();
    f.clear(0x3e0);
    assert!(matches!(
        m.present_frame(&mut f),
        Err(VideoError::UnsupportedObject { index: 0, .. })
    ));
    assert!(f.pixels().iter().all(|&p| p == rgb(0x3e0)));
    m.advance_cycles(FRAME);
    assert!(m.present_frame(&mut f).unwrap());
    assert_eq!(at(&f, 0, 1), rgb(31));
}

#[test]
fn hidden_preparation_errors_do_not_bypass_global_disable_or_forced_blank() {
    for control in [0x40, 0x10c0] {
        let mut m = scene();
        m.write16(OAM, 0xc00).unwrap();
        m.advance_cycles(40);
        m.write16(DISPCNT, control).unwrap();
        let f = finish(&mut m);
        assert_eq!(
            at(&f, 0, 1),
            if control & 0x80 != 0 {
                0xffffff
            } else {
                rgb(0x7c00)
            }
        );
    }
}

#[test]
fn zero_cycle_advances_and_failed_cpu_steps_do_not_prepare_rows() {
    let mut m = scene();
    m.advance_cycles(0);
    m.write16(OAM + 2, 16).unwrap();
    m.advance_cycles(39);
    m.write32(0x0300_0000, 0xf000_0000).unwrap();
    let mut machine = Machine::new(Cpu::new(0x0300_0000), m);
    assert!(machine.step().is_err());
    assert_eq!(machine.cycles(), 39);
    machine.memory_mut().write16(OAM + 2, 24).unwrap();
    let f = finish(machine.memory_mut());
    assert_eq!(at(&f, 16, 0), rgb(31));
    assert_eq!(at(&f, 24, 1), rgb(31));
}
