use super::*;

fn scene(horizontal: u16) -> Memory {
    let mut m = setup(160);
    m.write16(WIN0H, horizontal).unwrap();
    m
}

fn check_line(f: &Framebuffer, y: usize, inside: impl Fn(usize) -> bool) {
    for x in 0..WIDTH {
        assert_eq!(
            f.pixels()[y * WIDTH + x],
            if inside(x) { 0 } else { rgb(31) },
            "pixel={x},{y}"
        );
    }
}

#[test]
fn moving_left_after_entry_preserves_earlier_pixels_and_active_state() {
    let mut m = scene(0x0a50);
    m.advance_cycles(4 * 20 + 1);
    m.write8(WIN0H + 1, 40).unwrap();
    let f = finish(&mut m);
    check_line(&f, 0, |x| (10..80).contains(&x));
    check_line(&f, 1, |x| (40..80).contains(&x));
    let mut snapshot = Framebuffer::default();
    m.render_frame(&mut snapshot).unwrap();
    check_line(&snapshot, 0, |x| (40..80).contains(&x));
    assert_eq!(present(&m).pixels(), f.pixels());
}

#[test]
fn moving_left_behind_current_column_does_not_open_an_inactive_window() {
    let mut m = scene(0x6478);
    m.advance_cycles(4 * 20);
    m.write8(WIN0H + 1, 10).unwrap();
    let f = finish(&mut m);
    check_line(&f, 0, |_| false);
    check_line(&f, 1, |x| (10..120).contains(&x));
}

#[test]
fn missing_right_keeps_window_open_into_the_next_line() {
    let mut m = scene(0x0a50);
    m.advance_cycles(4 * 30);
    m.write8(WIN0H, 5).unwrap();
    let f = finish(&mut m);
    check_line(&f, 0, |x| x >= 10);
    check_line(&f, 1, |x| !(5..10).contains(&x));
}

#[test]
fn exact_four_cycle_sampling_boundaries_and_zero_cycle_advances_are_observable() {
    for elapsed in [79, 80, 81, 83] {
        let mut m = scene(0x6450);
        m.advance_cycles(elapsed);
        m.write16(WIN0H, 0x1450).unwrap();
        m.advance_cycles(0);
        let f = finish(&mut m);
        check_line(&f, 0, |x| elapsed <= 80 && (20..80).contains(&x));
        check_line(&f, 1, |x| (20..80).contains(&x));
    }
}

#[test]
fn equal_bounds_clear_at_the_edge_not_at_register_write() {
    for equal in [20, 40] {
        let mut m = scene(0x0ac8);
        m.advance_cycles(4 * 30);
        m.write16(WIN0H, equal * 0x101).unwrap();
        let f = finish(&mut m);
        check_line(&f, 0, |x| x >= 10 && (equal == 20 || x < 40));
        check_line(&f, 1, |x| equal == 20 && x < 20);
        check_line(&f, 2, |_| false);
    }
}

#[test]
fn offscreen_columns_including_late_hblank_set_next_lines_wrapping_state() {
    for left in [240, 251, 252, 255] {
        let mut m = scene((left << 8) | 10);
        let f = finish(&mut m);
        check_line(&f, 0, |_| false); // No previous line at startup.
        check_line(&f, 1, |x| x < 10);
        check_line(&f, 159, |x| x < 10);
    }
}

#[test]
fn hblank_writes_can_cancel_a_late_offscreen_edge_without_changing_captured_row() {
    let mut m = scene(0xff0a);
    m.advance_cycles(HB);
    m.write8(WIN0H + 1, 20).unwrap(); // X255 not reached; X20 already passed.
    let f = finish(&mut m);
    check_line(&f, 0, |_| false);
    check_line(&f, 1, |x| x >= 20);
    check_line(&f, 2, |x| !(10..20).contains(&x));
}

#[test]
fn independent_window_histories_preserve_win0_precedence() {
    let mut m = scene(0x0a50);
    m.write16(WIN1H, 0x1464).unwrap();
    m.write16(WIN1V, 160).unwrap();
    m.write16(DISPCNT, 0x6000).unwrap();
    m.write16(WININ, 0x2000).unwrap(); // WIN0 red, WIN1 black.
    m.advance_cycles(4 * 30);
    m.write32(WIN0H, 0x1464_3250).unwrap(); // Move WIN0's left edge only.
    let f = finish(&mut m);
    check_line(&f, 0, |x| (80..100).contains(&x));
    check_line(&f, 1, |x| (20..50).contains(&x) || (80..100).contains(&x));
    assert_eq!(m.read32(WIN0H).unwrap(), 0);
}

#[test]
fn disabled_windows_and_forced_blank_do_not_pause_horizontal_history() {
    for control in [0x4000, 0x2080] {
        let mut m = scene(0x0a50);
        m.write16(DISPCNT, control).unwrap();
        m.advance_cycles(4 * 30);
        m.write16(DISPCNT, 0x2000).unwrap();
        let f = finish(&mut m);
        // Enable/blank bits still sample at HBlank, unlike WIN0H comparator history.
        check_line(&f, 0, |x| (10..80).contains(&x));
    }
}

#[test]
fn hblank_dma_can_set_an_offscreen_edge_before_the_next_line() {
    let mut m = scene(0);
    for row in 0..160 {
        m.write16(0x0200_0000 + row * 2, 0xff0a).unwrap();
    }
    m.write32(DMA_BASE, 0x0200_0000).unwrap();
    m.write32(DMA_BASE + 4, WIN0H).unwrap();
    m.write32(DMA_BASE + 8, 0xa240_0001).unwrap();
    m.write8(HALTCNT, 0).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), m);
    machine.run_until_vblank(1000).unwrap();
    let f = present(machine.memory());
    check_line(&f, 0, |_| false);
    for y in 1..HEIGHT {
        check_line(&f, y, |x| x < 10);
    }
    assert!(machine.halted());
}

#[test]
fn cpu_store_crossing_a_comparator_uses_committed_bounds_not_final_instruction_time() {
    for after in [false, true] {
        let mut m = scene(0x6450);
        let code = [
            0xe3a0_0301, // MOV r0,#0x04000000
            0xe3a0_1b05, // MOV r1,#0x1400
            0xe381_1050, // ORR r1,r1,#0x50
            0xe1c0_14b0, // STRH r1,[r0,#0x40] (WIN0H)
        ];
        for (i, value) in code.into_iter().enumerate() {
            m.write32(0x0300_0000 + i as u32 * 4, value).unwrap();
        }
        let mut machine = Machine::new(Cpu::new(0x0300_0000), m);
        for _ in 0..3 {
            machine.step().unwrap();
        }
        let cycles = machine.cycles() as u32;
        machine
            .memory_mut()
            .advance_cycles(if after { 81 } else { 79 } - cycles);
        machine.step().unwrap();
        assert!(machine.cycles() > 80);
        let f = finish(machine.memory_mut());
        check_line(&f, 0, |x| !after && (20..80).contains(&x));
    }
}

#[test]
fn pixel_colors_still_sample_at_hblank_while_window_edges_keep_history() {
    let mut m = scene(0x0a50);
    m.advance_cycles(4 * 30);
    m.write8(WIN0H + 1, 40).unwrap();
    m.write16(PAL, 0x7c00).unwrap();
    let f = finish(&mut m);
    for x in 0..WIDTH {
        assert_eq!(
            f.pixels()[x],
            if (10..80).contains(&x) {
                0
            } else {
                rgb(0x7c00)
            }
        );
    }
}

#[test]
fn capture_enable_preserves_partial_line_history_and_hidden_lines_update_flags() {
    let mut m = scene(0x0a50);
    m.set_scanline_rendering(false);
    m.advance_cycles(4 * 30);
    m.write8(WIN0H + 1, 40).unwrap();
    m.set_scanline_rendering(true); // Before first HBlank, this frame is still complete.
    check_line(&finish(&mut m), 0, |x| (10..80).contains(&x));
    m.set_scanline_rendering(false);
    m.write16(WIN0H, 0xff0a).unwrap();
    m.advance_cycles(FRAME - VB);
    m.set_scanline_rendering(true);
    check_line(&finish(&mut m), 0, |x| x < 10);
}
