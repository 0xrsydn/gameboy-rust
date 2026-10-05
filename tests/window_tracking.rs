use gba_rust::{
    cpu::Cpu,
    display::{
        CYCLES_PER_FRAME as FRAME, CYCLES_PER_LINE as LINE, HBLANK_START as HB, VBLANK_START as VB,
    },
    dma::DMA_BASE,
    io::*,
    machine::Machine,
    memory::{Memory, PALETTE_START as PAL, ROM_START},
    video::{rgb555_to_rgb888 as rgb, Framebuffer, HEIGHT, WIDTH},
};

#[path = "window_tracking/horizontal.rs"]
mod horizontal;

fn setup(bounds: u16) -> Memory {
    let mut m = Memory::new(vec![]).unwrap();
    m.set_scanline_rendering(true);
    m.write16(DISPCNT, 0x2000).unwrap();
    m.write16(PAL, 31).unwrap();
    m.write16(WIN0H, 240).unwrap();
    m.write16(WIN1H, 240).unwrap();
    m.write16(WIN0V, bounds).unwrap();
    m.write16(WININ, 0x2020).unwrap();
    m.write16(WINOUT, 0).unwrap();
    m.write16(BLDCNT, 0xe0).unwrap(); // Darken backdrop only when effects are enabled.
    m.write16(BLDY, 16).unwrap();
    m
}

fn present(m: &Memory) -> Framebuffer {
    let mut f = Framebuffer::default();
    assert!(m.present_frame(&mut f).unwrap());
    f
}

fn finish(m: &mut Memory) -> Framebuffer {
    let position = m.display_position();
    let phase = u32::from(position.scanline) * LINE + u32::from(position.line_cycle);
    m.advance_cycles(VB - phase);
    present(m)
}

fn check_rows(f: &Framebuffer, inside: impl Fn(usize) -> bool) {
    for y in 0..HEIGHT {
        let expected = if inside(y) { 0 } else { rgb(31) };
        assert!(
            f.pixels()[y * WIDTH..(y + 1) * WIDTH]
                .iter()
                .all(|&p| p == expected),
            "row={y}"
        );
    }
}

#[test]
fn ordinary_static_windows_match_debug_snapshots() {
    for bounds in [0x00a0, 0x0130, 0x0405, 0x9fa0, 0x0000, 0x0505, 0x00ff] {
        let mut m = setup(bounds);
        let mut expected = Framebuffer::default();
        m.render_frame(&mut expected).unwrap();
        assert_eq!(
            finish(&mut m).pixels(),
            expected.pixels(),
            "bounds={bounds:#x}"
        );
    }
}

#[test]
fn moving_top_after_entry_does_not_close_an_active_window() {
    let mut m = setup(0x0210);
    m.advance_cycles(4 * LINE + HB);
    m.write8(WIN0V + 1, 12).unwrap();
    let f = finish(&mut m);
    check_rows(&f, |y| (2..16).contains(&y));
    let mut snapshot = Framebuffer::default();
    m.render_frame(&mut snapshot).unwrap();
    check_rows(&snapshot, |y| (12..16).contains(&y));
    assert_eq!(present(&m).pixels(), f.pixels()); // Snapshot rendering has no latch side effects.
}

#[test]
fn moving_top_behind_vcount_does_not_open_a_window() {
    let mut m = setup(0x0a14);
    m.advance_cycles(5 * LINE + HB);
    m.write8(WIN0V + 1, 2).unwrap();
    check_rows(&finish(&mut m), |_| false);
    m.advance_cycles(FRAME - VB);
    check_rows(&finish(&mut m), |y| (2..20).contains(&y));
}

#[test]
fn missing_bottom_keeps_window_open_across_frame_wrap() {
    let mut m = setup(0x0214);
    m.advance_cycles(5 * LINE + HB);
    m.write8(WIN0V, 4).unwrap();
    check_rows(&finish(&mut m), |y| y >= 2);
    m.advance_cycles(FRAME - VB);
    check_rows(&finish(&mut m), |y| y < 4); // No unconditional line-zero reset.
}

#[test]
fn writes_before_line_start_take_effect_but_writes_after_it_do_not_replay_edges() {
    for before in [false, true] {
        let mut m = setup(0x1420);
        m.advance_cycles(5 * LINE - u32::from(before));
        m.write16(WIN0V, 0x0508).unwrap();
        m.advance_cycles(0); // Does not trigger a comparator.
        check_rows(&finish(&mut m), |y| before && (5..8).contains(&y));
    }
}

#[test]
fn hblank_writes_cannot_replay_the_current_rows_comparator() {
    let mut m = setup(0x0208);
    m.advance_cycles(3 * LINE + HB);
    m.write16(WIN0V, 0x0203).unwrap(); // Bottom already missed this row.
    check_rows(&finish(&mut m), |y| y >= 2);
}

#[test]
fn hidden_line_edges_run_and_unreachable_edges_do_not_fire() {
    for top in [160, 200, 227, 228, 255] {
        let mut m = setup((top << 8) | 8);
        check_rows(&finish(&mut m), |_| false); // Reset starts outside; top not reached yet.
        m.advance_cycles(FRAME - VB);
        check_rows(&finish(&mut m), |y| top < 228 && y < 8);
    }
}

#[test]
fn equal_bounds_clear_only_at_the_matching_edge_and_bottom_wins() {
    let mut m = setup(0x0214);
    m.advance_cycles(4 * LINE);
    m.write16(WIN0V, 0x0808).unwrap();
    check_rows(&finish(&mut m), |y| (2..8).contains(&y));

    let mut m = setup(0x0214);
    m.advance_cycles(4 * LINE);
    m.write16(WIN0V, 0xffff).unwrap(); // Neither edge is reachable by VCOUNT.
    check_rows(&finish(&mut m), |y| y >= 2);
    m.advance_cycles(FRAME - VB);
    check_rows(&finish(&mut m), |_| true);
}

#[test]
fn window_disable_and_forced_blank_do_not_pause_comparators() {
    for control in [0x4000, 0x2080] {
        let mut m = setup(0x0210);
        m.write16(DISPCNT, control).unwrap();
        m.advance_cycles(5 * LINE);
        m.write16(DISPCNT, 0x2000).unwrap();
        let f = finish(&mut m);
        for y in 0..HEIGHT {
            let expected = if y < 5 && control & 0x80 != 0 {
                0xffffff
            } else if (5..16).contains(&y) {
                0
            } else {
                rgb(31)
            };
            assert_eq!(
                f.pixels()[y * WIDTH],
                expected,
                "control={control:#x} row={y}"
            );
        }
    }
}

#[test]
fn win0_and_win1_keep_independent_state_and_win0_keeps_precedence() {
    let mut m = setup(0x0206);
    m.write32(WIN0V, 0x0408_0206).unwrap(); // Independent byte merging for both windows.
    m.write16(DISPCNT, 0x6000).unwrap();
    m.write16(WININ, 0x2000).unwrap(); // WIN0 allows red; WIN1 darkens to black.
    m.advance_cycles(3 * LINE + HB);
    m.write8(WIN0V + 1, 5).unwrap(); // Does not close WIN0 before row5.
    m.advance_cycles(LINE);
    m.write8(WIN1V + 1, 7).unwrap(); // Does not close WIN1 before row7.
    check_rows(&finish(&mut m), |y| (6..8).contains(&y));
    assert_eq!(m.read32(WIN0V).unwrap(), 0); // Registers remain write-only.
}

#[test]
fn hblank_bounds_and_mask_writes_apply_to_subsequent_rows() {
    let mut m = setup(0x020a);
    m.advance_cycles(4 * LINE + HB);
    m.write16(WIN0H, 0x0a14).unwrap();
    m.advance_cycles(LINE);
    m.write16(WININ, 0).unwrap();
    let f = finish(&mut m);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let dark = (2..5).contains(&y) || (y == 5 && (10..20).contains(&x));
            assert_eq!(f.pixels()[y * WIDTH + x], if dark { 0 } else { rgb(31) });
        }
    }
}

#[test]
fn hblank_dma_programs_future_edges_without_recomputing_interval_membership() {
    let mut m = setup(0x1020);
    for y in 0..160 {
        m.write16(0x0200_0000 + y * 2, if y == 0 { 0x0104 } else { 0xc804 })
            .unwrap();
    }
    m.write32(DMA_BASE, 0x0200_0000).unwrap();
    m.write32(DMA_BASE + 4, WIN0V).unwrap();
    m.write32(DMA_BASE + 8, 0xa240_0001).unwrap();
    m.write8(HALTCNT, 0).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), m);
    machine.run_until_vblank(1000).unwrap();
    check_rows(&present(machine.memory()), |y| (1..4).contains(&y));
    assert!(machine.halted());
    assert_eq!(machine.memory().read16(DMA_BASE + 10).unwrap(), 0xa240);
}

#[test]
fn capture_disabled_still_tracks_hidden_edges_and_enabling_does_not_reset_state() {
    let mut m = setup(0xc808);
    m.set_scanline_rendering(false);
    m.advance_cycles(FRAME); // Hidden row200 opens the window, retained at row0.
    m.set_scanline_rendering(true);
    m.set_scanline_rendering(true);
    check_rows(&finish(&mut m), |y| y < 8);
}
