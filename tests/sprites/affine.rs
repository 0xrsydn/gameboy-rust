use super::*;

fn matrix(m: &mut Memory, group: u32, values: [i16; 4]) {
    for (i, value) in values.into_iter().enumerate() {
        m.write16(OAM + group * 32 + 6 + i as u32 * 8, value as u16)
            .unwrap();
    }
}

fn texel(x: usize, y: usize) -> u16 {
    if (x + y) % 7 == 0 {
        0
    } else {
        ((x + 3 * y) % 15 + 1) as u16
    }
}

fn pattern(m: &mut Memory, w: usize, h: usize, eight: bool, one: bool) {
    let mut bytes = [0_u8; 0x8000];
    for y in 0..h {
        for x in 0..w {
            let slot = (y / 8)
                * if one {
                    w / 8 * if eight { 2 } else { 1 }
                } else {
                    32
                }
                + (x / 8) * if eight { 2 } else { 1 };
            let p = (y % 8) * 8 + x % 8;
            let address = slot * 32 + if eight { p } else { p / 2 };
            bytes[address] |= (texel(x, y) as u8) << if eight { 0 } else { (p % 2) * 4 };
        }
    }
    for (i, pair) in bytes.chunks_exact(2).enumerate() {
        m.write16(
            VRAM + 0x10000 + i as u32 * 2,
            u16::from_le_bytes([pair[0], pair[1]]),
        )
        .unwrap();
    }
    for i in 1..16 {
        m.write16(PAL + 0x200 + i * 2, i as u16 * 0x421).unwrap();
    }
}

// Independent floating-point reference. Every coefficient is an exact multiple
// of 1/256, so these small products are exact in f64. Floor defines negative rounding.
fn check_reference(
    m: &Memory,
    dimensions: (usize, usize),
    origin: (usize, usize),
    doubled: bool,
    values: [i16; 4],
) {
    let (w, h) = dimensions;
    let (cw, ch) = if doubled { (w * 2, h * 2) } else { (w, h) };
    let f = render(m);
    for y in 0..160 {
        for x in 0..240 {
            let lx = (x + 512 - origin.0) % 512;
            let ly = (y + 256 - origin.1) % 256;
            let mut color = rgb555_to_rgb888(0x4210);
            if lx < cw && ly < ch {
                let dx = lx as f64 - cw as f64 / 2.0;
                let dy = ly as f64 - ch as f64 / 2.0;
                let tx = (f64::from(values[0]) / 256.0 * dx
                    + f64::from(values[1]) / 256.0 * dy
                    + w as f64 / 2.0)
                    .floor();
                let ty = (f64::from(values[2]) / 256.0 * dx
                    + f64::from(values[3]) / 256.0 * dy
                    + h as f64 / 2.0)
                    .floor();
                if tx >= 0.0 && ty >= 0.0 && tx < w as f64 && ty < h as f64 {
                    let index = texel(tx as usize, ty as usize);
                    if index != 0 {
                        color = rgb555_to_rgb888(index * 0x421);
                    }
                }
            }
            assert_eq!(
                f.pixels()[y * WIDTH + x],
                color,
                "pixel {x},{y}, size {w}x{h}, double={doubled}, matrix={values:?}"
            );
        }
    }
}

#[test]
fn identity_matches_regular_sprites_for_all_shapes_sizes_depths_and_layouts() {
    let sizes = [
        [(8, 8), (16, 16), (32, 32), (64, 64)],
        [(16, 8), (32, 8), (32, 16), (64, 32)],
        [(8, 16), (8, 32), (16, 32), (32, 64)],
    ];
    for (shape, row) in sizes.iter().enumerate() {
        for (size, &(w, h)) in row.iter().enumerate() {
            for eight in [false, true] {
                for one in [false, true] {
                    let mut m = setup();
                    m.write16(DISPCNT, if one { 0x1040 } else { 0x1000 })
                        .unwrap();
                    pattern(&mut m, w, h, eight, one);
                    let a = (shape as u16) << 14 | if eight { 0x2000 } else { 0 } | 9;
                    let b = (size as u16) << 14 | 11;
                    obj(&mut m, 0, a, b, 0);
                    let regular = render(&m);
                    matrix(&mut m, 31, [256, 0, 0, 256]);
                    obj(&mut m, 0, a | 0x100, b | (31 << 9), 0); // High group bits are not flip flags.
                    assert_eq!(render(&m).pixels(), regular.pixels());
                }
            }
        }
    }
}

#[test]
fn rotations_shears_reflections_and_fractional_scales_match_reference() {
    let transforms = [
        [0, 256, -256, 0],
        [0, -256, 256, 0],
        [-256, 0, 0, -256],
        [181, 181, -181, 181],
        [256, 128, 0, 256],
        [-256, 0, 0, 256],
        [128, 0, 0, 128],
        [512, 0, 0, 512],
        [512, 0, 0, 128],
        [255, 1, -1, 257],
    ];
    for eight in [false, true] {
        for one in [false, true] {
            for doubled in [false, true] {
                let mut m = setup();
                m.write16(DISPCNT, if one { 0x1040 } else { 0x1000 })
                    .unwrap();
                pattern(&mut m, 16, 8, eight, one);
                obj(
                    &mut m,
                    0,
                    0x4100 | if doubled { 0x200 } else { 0 } | if eight { 0x2000 } else { 0 } | 17,
                    23,
                    0,
                );
                for values in transforms {
                    matrix(&mut m, 0, values);
                    check_reference(&m, (16, 8), (23, 17), doubled, values);
                }
            }
        }
    }
}

#[test]
fn double_size_changes_canvas_and_center_without_changing_texture_size() {
    let mut m = setup();
    pattern(&mut m, 8, 8, false, true);
    for doubled in [false, true] {
        obj(
            &mut m,
            0,
            0x100 | if doubled { 0x200 } else { 0 } | 20,
            30,
            0,
        );
        for values in [[256, 0, 0, 256], [128, 0, 0, 128]] {
            matrix(&mut m, 0, values);
            check_reference(&m, (8, 8), (30, 20), doubled, values);
        }
    }
    // Identity double-size: texture starts four pixels inside the enlarged box.
    matrix(&mut m, 0, [256, 0, 0, 256]);
    assert_eq!(pixel(&m, 30, 20), rgb555_to_rgb888(0x4210));
    assert_eq!(pixel(&m, 35, 24), rgb555_to_rgb888(texel(1, 0) * 0x421));
}

#[test]
fn zero_singular_and_extreme_signed_matrices_do_not_overflow_or_invert() {
    let mut m = setup();
    pattern(&mut m, 64, 64, true, true);
    obj(&mut m, 0, 0x2300, 0xc000, 0);
    for values in [
        [0; 4],
        [256, 0, 0, 0],
        [32767, -32768, -32768, 32767],
        [-1, 1, 1, -1],
        [1, 0, 0, 1],
    ] {
        matrix(&mut m, 0, values);
        check_reference(&m, (64, 64), (0, 0), true, values);
    }
}

#[test]
fn all_32_matrix_groups_are_addressed_independently_and_can_be_shared() {
    let mut m = setup();
    pattern(&mut m, 8, 8, false, true);
    for group in 0..32 {
        // Distinct coefficients detect both incorrect matrix stride and component stride.
        let values = [
            256 + group as i16,
            group as i16 * 3,
            -(group as i16) * 2,
            128,
        ];
        matrix(&mut m, group, values);
    }
    for group in 0..32 {
        let values = [
            256 + group as i16,
            group as i16 * 3,
            -(group as i16) * 2,
            128,
        ];
        obj(&mut m, 0, 0x100, (group as u16) << 9, 0);
        check_reference(&m, (8, 8), (0, 0), false, values);
    }
    matrix(&mut m, 31, [256, 0, 0, 256]);
    obj(&mut m, 0, 0x100, 31 << 9, 0);
    obj(&mut m, 127, 0x100, (31 << 9) | 16, 0);
    let f = render(&m);
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(f.pixels()[y * WIDTH + x], f.pixels()[y * WIDTH + x + 16]);
        }
    }
    // Changes to one shared matrix are visible to both sprites on the next snapshot.
    matrix(&mut m, 31, [0; 4]);
    let f = render(&m);
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(f.pixels()[y * WIDTH + x], f.pixels()[y * WIDTH + x + 16]);
        }
    }
}

#[test]
fn affine_canvas_wraps_coordinates_and_clips_without_restarting_transform() {
    let mut m = setup();
    pattern(&mut m, 32, 64, false, true);
    let values = [181, 181, -181, 181];
    matrix(&mut m, 0, values);
    for (x, y) in [(500, 240), (220, 145), (255, 170)] {
        obj(&mut m, 0, 0x8300 | y as u16, 0xc000 | x as u16, 0);
        check_reference(&m, (32, 64), (x, y), true, values);
    }
}

#[test]
fn affine_transparent_edges_reveal_later_objects_and_obey_bg_priority() {
    let mut m = setup();
    fill(&mut m, 0, 32, 0x1111);
    fill(&mut m, 1, 32, 0x2222);
    matrix(&mut m, 0, [512, 0, 0, 512]);
    obj(&mut m, 0, 0x100, 0, 0x400);
    obj(&mut m, 1, 0, 0, 0x401); // Equal priority retains the first opaque OBJ.
    assert_eq!(pixel(&m, 0, 0), 0x00ff00); // Outside transformed texture, later OBJ visible.
    assert_eq!(pixel(&m, 4, 4), 0xff0000);
    m.write16(DISPCNT, 0x1140).unwrap();
    m.write16(BG0CNT, 0x1000).unwrap();
    for address in (0..32).step_by(2) {
        m.write16(VRAM + address, 0x3333).unwrap();
    }
    m.write16(PAL + 6, 0x7c00).unwrap();
    assert_eq!(pixel(&m, 4, 4), 0x0000ff); // BG priority0 covers selected OBJ priority1.
    assert_eq!(pixel(&m, 0, 0), 0x0000ff);
    m.write16(BG0CNT, 0x1001).unwrap();
    assert_eq!(pixel(&m, 0, 0), 0x00ff00); // OBJ ties BG and wins at transparent edge.
}

#[test]
fn mode3_affine_sprites_keep_bitmap_tile_restrictions() {
    let mut m = setup();
    m.write16(DISPCNT, 0x1043).unwrap();
    matrix(&mut m, 0, [128, 0, 0, 128]);
    fill(&mut m, 0, 32, 0x1111);
    fill(&mut m, 512, 32, 0x2222);
    obj(&mut m, 0, 0x300, 0, 0);
    assert_eq!(pixel(&m, 0, 0), rgb555_to_rgb888(0x4210));
    obj(&mut m, 0, 0x300, 0, 512);
    assert_eq!(pixel(&m, 0, 0), 0x00ff00);
    assert_eq!(pixel(&m, 15, 15), 0x00ff00);
    assert_eq!(pixel(&m, 16, 16), rgb555_to_rgb888(0x4210));
}
