use super::*;

fn scene(a: u16, b: u16) -> Memory {
    let mut m = memory();
    m.write16(DISPCNT, 0x1040).unwrap();
    object(&mut m, 0, a | 7, b | 5 | 0x4000, 0); // 16x16 at (5,7).
    for y in 0..16 {
        for x in (0..16).step_by(2) {
            let tile = y / 8 * 2 + x / 8;
            let address = VRAM + 0x10000 + tile * 64 + y % 8 * 8 + x % 8;
            let first = 1 + (x + y * 5) % 254;
            let second = 1 + (x + 1 + y * 5) % 254;
            m.write16(address, (first | second << 8) as u16).unwrap();
        }
    }
    m
}

#[test]
fn regular_obj_uses_screen_grid_clamps_partial_first_blocks_and_preserves_flips() {
    for flips in [0, 0x1000, 0x2000, 0x3000] {
        let mut m = scene(0x2000, flips);
        let base = render(&m);
        m.write16(OAM, 0x3007).unwrap(); // 8bpp plus mosaic, y7.
        for width in 1..=16 {
            let height = 17 - width;
            m.write16(MOSAIC, (((height - 1) << 12) | ((width - 1) << 8)) as u16)
                .unwrap();
            let f = render(&m);
            for y in 0..32 {
                for x in 0..32 {
                    let expected = if (5..21).contains(&x) && (7..23).contains(&y) {
                        at(
                            &base,
                            (x / width * width).max(5),
                            (y / height * height).max(7),
                        )
                    } else {
                        0
                    };
                    assert_eq!(
                        at(&f, x, y),
                        expected,
                        "flip={flips:x} size={width}x{height} at={x},{y}"
                    );
                }
            }
        }
    }
}

#[test]
fn affine_and_double_size_mosaic_repeat_transformed_samples() {
    for doubled in [false, true] {
        let mut m = scene(0x2100 | if doubled { 0x200 } else { 0 }, 0);
        let matrix: [i16; 4] = if doubled {
            [64, 32, -32, 64]
        } else {
            [128, 64, -64, 128]
        };
        for (i, value) in matrix.into_iter().enumerate() {
            m.write16(OAM + 6 + i as u32 * 8, value as u16).unwrap();
        }
        let base = render(&m);
        m.write16(OAM, 0x3107 | if doubled { 0x200 } else { 0 })
            .unwrap();
        m.write16(MOSAIC, 0x4500).unwrap(); // OBJ6x5; BG1x1.
        let f = render(&m);
        let size = if doubled { 32 } else { 16 };
        for y in 0..48 {
            for x in 0..48 {
                let expected = if (5..5 + size).contains(&x) && (7..7 + size).contains(&y) {
                    at(&base, (x / 6 * 6).max(5), (y / 5 * 5).max(7))
                } else {
                    0
                };
                assert_eq!(at(&f, x, y), expected, "double={doubled} at={x},{y}");
            }
        }
    }
}

#[test]
fn obj_flag_and_high_byte_are_independent_and_vertical_sampling_wraps_at_screen_top() {
    let mut m = scene(0x2000, 0);
    let base = render(&m);
    m.write16(MOSAIC, 0xffff).unwrap();
    assert_eq!(render(&m).pixels(), base.pixels()); // Flag disabled.
    m.write16(OAM, 0x3007).unwrap();
    m.write8(MOSAIC + 1, 0).unwrap(); // BG15 bits do not affect OBJ.
    assert_eq!(render(&m).pixels(), base.pixels());
    object(&mut m, 0, 0x20fc, 0x41fc, 0); // 16x16, wraps from (-4,-4).
    let base = render(&m);
    m.write16(OAM, 0x30fc).unwrap();
    m.write16(MOSAIC, 0x3400).unwrap();
    let f = render(&m);
    for y in 0..12 {
        for x in 0..12 {
            assert_eq!(at(&f, x, y), at(&base, x / 5 * 5, y / 4 * 4));
        }
    }
    assert_eq!(at(&f, 12, 0), 0);
    assert_eq!(at(&f, 0, 12), 0);
}

#[test]
fn transparent_samples_repeat_but_non_mosaic_samples_restart_the_latch() {
    let mut m = memory();
    m.write16(DISPCNT, 0x1040).unwrap();
    m.write16(MOSAIC, 0x0300).unwrap(); // OBJ4x1.
    m.write16(PAL + 0x202, 31).unwrap();
    fill(&mut m, VRAM + 0x10000, 32, 0x1010); // First sample transparent, second red.
    object(&mut m, 0, 0x1000, 0, 0);
    assert!(render(&m).pixels().iter().all(|&p| p == 0));
    m.write16(VRAM + 0x10000, 0x0101).unwrap();
    let f = render(&m);
    for x in 0..4 {
        assert_eq!(at(&f, x, 0), rgb(31));
    }
    // A lower-priority non-mosaic sprite fills transparent texels before mosaic.
    // Its non-mosaic metadata makes those pixels latch immediately.
    fill(&mut m, VRAM + 0x10020, 32, 0x2222);
    m.write16(PAL + 0x204, 0x3e0).unwrap();
    object(&mut m, 1, 0, 0, 0x401);
    let f = render(&m);
    assert_eq!(at(&f, 0, 0), rgb(31));
    assert_eq!(at(&f, 1, 0), rgb(0x3e0));
}

#[test]
fn horizontal_latch_restarts_for_non_mosaic_and_higher_priority_pixels() {
    let mut m = memory();
    m.write16(DISPCNT, 0x1040).unwrap();
    m.write16(MOSAIC, 0x0f00).unwrap();
    for (tile, color) in [(0, 31), (1, 0x3e0), (2, 0x7c00)] {
        fill(
            &mut m,
            VRAM + 0x10000 + tile * 32,
            32,
            (tile as u16 + 1) * 0x1111,
        );
        m.write16(PAL + 0x202 + tile * 2, color).unwrap();
    }
    object(&mut m, 0, 0x1000, 1, 0x800);
    object(&mut m, 1, 0x1000, 3, 0x401);
    object(&mut m, 2, 0, 5, 2);
    let f = render(&m);
    assert_eq!(at(&f, 0, 0), 0);
    assert_eq!(at(&f, 1, 0), rgb(31));
    assert_eq!(at(&f, 3, 0), rgb(0x3e0));
    assert_eq!(at(&f, 5, 0), rgb(0x7c00));
    assert_eq!(at(&f, 13, 0), 0); // Leaving the canvas resets the latch.
}

#[test]
fn transparent_higher_priority_obj_changes_metadata_without_replacing_color() {
    let mut m = memory();
    m.write16(DISPCNT, 0x1140).unwrap();
    m.write16(BG0CNT, 0x1001).unwrap(); // BG priority1.
    fill(&mut m, VRAM, 32, 0x3333);
    m.write16(PAL + 6, 0x7c00).unwrap();
    fill(&mut m, VRAM + 0x10000, 32, 0x1111);
    m.write16(PAL + 0x202, 31).unwrap();
    object(&mut m, 0, 0x400, 0, 0x800); // Semi-transparent red, priority2.
    object(&mut m, 1, 0x1000, 0, 0x401); // Transparent tile1, priority1, mosaic.
    fill(&mut m, VRAM + 0x10040, 32, 0x2222);
    object(&mut m, 2, 0, 0, 0x402); // Same priority as metadata; cannot replace red.
    m.write16(MOSAIC, 0x0300).unwrap();
    m.write16(BLDCNT, 0x100).unwrap();
    m.write16(BLDALPHA, 0x0808).unwrap();
    assert_eq!(at(&render(&m), 0, 0), rgb(0x3c0f)); // Alpha flag retained, priority now ties BG.
}

#[test]
fn object_window_ignores_mosaic_and_window_edges_do_not_quantize() {
    let mut m = memory();
    m.write16(DISPCNT, 0x9100).unwrap();
    m.write16(BG0CNT, 0x1000).unwrap();
    fill(&mut m, VRAM, 32, 0x1111);
    m.write16(PAL + 2, 31).unwrap();
    fill(&mut m, VRAM + 0x10000, 32, 0x1010);
    object(&mut m, 0, 0x1800, 0, 0);
    m.write16(MOSAIC, 0xffff).unwrap();
    m.write16(WINOUT, 0x0100).unwrap();
    let f = render(&m);
    for x in 0..8 {
        assert_eq!(at(&f, x, 0), if x % 2 == 0 { 0 } else { rgb(31) });
    }
    m.write16(VRAM + 0x10004, 0x0101).unwrap(); // Next source row remains distinct.
    assert_eq!(at(&render(&m), 0, 1), rgb(31));
}

#[test]
fn mosaic_latch_precedes_window_masking_and_semi_transparent_blending() {
    let mut m = memory();
    m.write16(DISPCNT, 0x3100).unwrap();
    m.write16(BG0CNT, 0x1000).unwrap();
    fill(&mut m, VRAM, 32, 0x2222);
    m.write16(PAL + 4, 0x7c00).unwrap();
    m.write16(PAL + 0x202, 31).unwrap();
    fill(&mut m, VRAM + 0x10000, 32, 0x0101);
    object(&mut m, 0, 0x1400, 0, 0);
    m.write16(MOSAIC, 0x0300).unwrap();
    m.write16(WIN0H, 0x0001).unwrap();
    m.write16(WIN0V, 0x0008).unwrap();
    m.write16(WININ, 1).unwrap(); // First pixel masks OBJ but must not reset its latch.
    m.write16(WINOUT, 0x31).unwrap();
    m.write16(BLDCNT, 0x100).unwrap();
    m.write16(BLDALPHA, 0x0808).unwrap();
    let f = render(&m);
    assert_eq!(at(&f, 0, 0), rgb(0x7c00));
    for x in 1..4 {
        assert_eq!(at(&f, x, 0), rgb(0x3c0f));
    }
}
