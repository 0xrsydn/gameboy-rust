use gba_rust::{
    io::*,
    memory::{Memory, MemoryError, OAM_START as OAM, PALETTE_START as PAL, VRAM_START as VRAM},
    video::{rgb555_to_rgb888, Framebuffer, VideoError, WIDTH},
};

fn setup(bg: u32, size: u16, wrap: bool) -> Memory {
    let mut m = Memory::new(vec![]).unwrap();
    m.write16(DISPCNT, 2 | (0x100 << bg)).unwrap();
    m.write16(
        BG0CNT + bg * 2,
        0x1000 | (size << 14) | if wrap { 0x2000 } else { 0 },
    )
    .unwrap();
    transform(&mut m, bg, [256, 0, 0, 256], [0, 0]);
    m.write16(PAL, 0x4000).unwrap();
    for i in 1..=255 {
        m.write16(PAL + i * 2, i as u16 * 31).unwrap();
    }
    // All 256 byte-sized tile numbers, with an asymmetric indexed pattern.
    for tile in 0..256 {
        for y in 0..8 {
            for x in (0..8).step_by(2) {
                let lo = index(tile, x, y);
                let hi = index(tile, x + 1, y);
                m.write16(
                    VRAM + tile * 64 + y * 8 + x,
                    u16::from(lo) | (u16::from(hi) << 8),
                )
                .unwrap();
            }
        }
    }
    let tiles = 16_u32 << size;
    for y in 0..tiles {
        for x in (0..tiles).step_by(2) {
            m.write16(
                VRAM + 0x8000 + y * tiles + x,
                u16::from(tile_number(x, y)) | (u16::from(tile_number(x + 1, y)) << 8),
            )
            .unwrap();
        }
    }
    m
}

fn tile_number(x: u32, y: u32) -> u8 {
    (x.wrapping_mul(17) + y.wrapping_mul(31)) as u8
}
fn index(tile: u32, x: u32, y: u32) -> u8 {
    (tile + x + 3 * y) as u8
}
fn transform(m: &mut Memory, bg: u32, coeff: [i16; 4], origin: [i32; 2]) {
    let base = BG2PA + (bg - 2) * 16;
    for (i, v) in coeff.into_iter().enumerate() {
        m.write16(base + i as u32 * 2, v as u16).unwrap();
    }
    for (i, v) in origin.into_iter().enumerate() {
        m.write32(base + 8 + i as u32 * 4, v as u32).unwrap();
    }
}
fn frame(m: &Memory) -> Framebuffer {
    let mut f = Framebuffer::default();
    m.render_frame(&mut f).unwrap();
    f
}
fn pixel(m: &Memory, x: usize, y: usize) -> u32 {
    frame(m).pixels()[y * WIDTH + x]
}

fn reference(m: &Memory, size: u16, wrap: bool, coeff: [i16; 4], origin: [i32; 2]) {
    let f = frame(m);
    let size = 128_i64 << size;
    for y in 0..160 {
        for x in 0..240 {
            // Wide-integer reference with explicit 28-bit wrapping and floor division.
            let coord = |r: i32, a: i16, b: i16| {
                let v = i64::from(r) + i64::from(a) * x as i64 + i64::from(b) * y as i64;
                let signed = (v + (1 << 27)).rem_euclid(1 << 28) - (1 << 27);
                signed.div_euclid(256)
            };
            let mut tx = coord(origin[0], coeff[0], coeff[1]);
            let mut ty = coord(origin[1], coeff[2], coeff[3]);
            let mut color = 0x4000;
            if wrap || (tx >= 0 && ty >= 0 && tx < size && ty < size) {
                tx = tx.rem_euclid(size);
                ty = ty.rem_euclid(size);
                let t = tile_number(tx as u32 / 8, ty as u32 / 8);
                let i = index(u32::from(t), tx as u32 % 8, ty as u32 % 8);
                if i != 0 {
                    color = u16::from(i) * 31;
                }
            }
            assert_eq!(
                f.pixels()[y * WIDTH + x],
                rgb555_to_rgb888(color),
                "pixel {x},{y}, size {size}, origin={origin:?}, matrix={coeff:?}"
            );
        }
    }
}

#[test]
fn affine_matrices_sizes_wrapping_and_signed_origins_match_wide_reference() {
    for bg in [2, 3] {
        for size in 0..4 {
            for wrap in [false, true] {
                let mut m = setup(bg, size, wrap);
                for (coeff, origin) in [
                    ([256, 0, 0, 256], [0, 0]),
                    ([0, 256, -256, 0], [-1, 128 * 256]),
                    ([181, 181, -181, 181], [-173, 17551]),
                    ([128, 0, 0, 512], [127, -129]),
                    ([256, 127, 1, -256], [0x07ff_ffff, 0x0800_0000]),
                    ([32767, -32768, -32768, 32767], [-123456, 123456]),
                    ([0; 4], [23 * 256, 17 * 256]),
                ] {
                    transform(&mut m, bg, coeff, origin);
                    reference(&m, size, wrap, coeff, origin);
                }
            }
        }
    }
}

#[test]
fn write_only_registers_merge_bytes_halfwords_and_words_and_mask_reference_top_bits() {
    let mut m = setup(2, 1, true);
    m.write32(BG2PA, 0xff80_0180).unwrap(); // A=1.5, B=-0.5
    m.write8(BG2PA, 0x40).unwrap(); // A=1.25, retains high byte
    m.write32(BG2PC, 0x0100_0000).unwrap();
    m.write32(BG2X, 0xffff_ffff).unwrap();
    m.write8(BG2X, 0x80).unwrap(); // -0.5
    m.write16(BG2Y, 0x0101).unwrap();
    m.write16(BG2Y + 2, 0xf000).unwrap(); // Unused high nibble must be zero internally.
    reference(&m, 1, true, [320, -128, 0, 256], [-128, 257]);
    for address in (BG2PA..=BG3Y).step_by(4) {
        assert_eq!(m.read32(address).unwrap(), 0);
    }
    assert_eq!(
        m.read16(0x0400_0058),
        Err(MemoryError::Unmapped(0x0400_0058))
    );
    assert_eq!(m.cycles(), 0);
}

#[test]
fn affine_bg_ignores_text_scroll_and_color_depth_bit() {
    let mut m = setup(2, 1, true);
    let before = frame(&m);
    m.write32(BG2HOFS, u32::MAX).unwrap();
    m.write16(BG2CNT, 0x7080).unwrap(); // Same map/size/wrap, color bit set.
    assert_eq!(frame(&m).pixels(), before.pixels());
}

#[test]
fn affine_byte_maps_are_flat_rows_use_tile255_and_have_no_flip_or_bank_bits() {
    let mut m = setup(2, 2, false);
    m.write16(VRAM + 0x8000 + 64, 0x00ff).unwrap(); // (tileX=0,tileY=1), not next text screen block.
    assert_eq!(pixel(&m, 0, 8), rgb555_to_rgb888(255 * 31));
    assert_eq!(pixel(&m, 1, 8), rgb555_to_rgb888(0x4000)); // Tile255 texel1 wraps to palette index0.
    assert_eq!(pixel(&m, 8, 8), rgb555_to_rgb888(0x4000)); // Adjacent map byte selects tile0.
}

#[test]
fn affine_character_base_and_last_tile_reach_the_end_of_bg_vram() {
    let mut m = setup(2, 0, false);
    m.write16(BG2CNT, 0x100c).unwrap(); // Character base3, map16.
    m.write16(VRAM + 0x8000, 0x00ff).unwrap();
    m.write16(VRAM + 0xfffe, 0x0201).unwrap();
    transform(&mut m, 2, [256, 0, 0, 256], [6 * 256, 7 * 256]);
    assert_eq!(pixel(&m, 0, 0), rgb555_to_rgb888(31));
    assert_eq!(pixel(&m, 1, 0), rgb555_to_rgb888(62));
}

#[test]
fn mode1_composes_two_text_layers_and_one_affine_layer_with_priority_ties() {
    let mut m = setup(2, 1, true);
    m.write16(DISPCNT, 0x701).unwrap();
    m.write16(BG0CNT, 0x1804).unwrap(); // Text tile base1, map24, priority0.
    m.write16(BG1CNT, 0x1904).unwrap();
    m.write16(VRAM + 0x4000, 0x2222).unwrap();
    m.write16(VRAM + 0xc800, 1).unwrap();
    m.write16(VRAM + 0x4020, 0x3333).unwrap();
    m.write16(VRAM, 0x0101).unwrap(); // BG2 affine nonzero index1.
    assert_eq!(pixel(&m, 0, 0), rgb555_to_rgb888(2 * 31)); // BG0 wins tie.
    m.write16(BG0CNT, 0x1807).unwrap();
    assert_eq!(pixel(&m, 0, 0), rgb555_to_rgb888(3 * 31)); // BG1 wins tie over BG2.
    m.write16(BG1CNT, 0x1907).unwrap();
    assert_eq!(pixel(&m, 0, 0), rgb555_to_rgb888(31));
    m.write16(VRAM, 0).unwrap(); // Affine palette index0 reveals text BG0.
    assert_eq!(pixel(&m, 0, 0), rgb555_to_rgb888(2 * 31));
}

#[test]
fn mode2_bg3_has_independent_matrix_reference_and_priority() {
    let mut m = setup(2, 1, true);
    m.write16(DISPCNT, 0xc02).unwrap();
    m.write16(BG3CNT, 0x7000).unwrap();
    transform(&mut m, 2, [0; 4], [256, 0]);
    transform(&mut m, 3, [0; 4], [512, 0]);
    assert_eq!(pixel(&m, 0, 0), rgb555_to_rgb888(31)); // BG2 wins tie.
    m.write16(BG2CNT, 0x7001).unwrap();
    assert_eq!(pixel(&m, 0, 0), rgb555_to_rgb888(62));
    m.write16(PAL + 4, 0).unwrap();
    assert_eq!(pixel(&m, 0, 0), 0); // Nonzero index with black color is opaque.
}

#[test]
fn mode3_uses_affine_matrix_ignores_overflow_and_keeps_black_opaque() {
    let mut m = Memory::new(vec![]).unwrap();
    m.write16(DISPCNT, 0x403).unwrap();
    m.write16(PAL, 0x03e0).unwrap();
    m.write16(VRAM, 0).unwrap();
    m.write16(VRAM + 2, 0x001f).unwrap();
    m.write16(VRAM + (159 * 240 + 239) * 2, 0x7fff).unwrap();
    transform(&mut m, 2, [256, 0, 0, 256], [-256, 0]);
    m.write16(BG2CNT, 0x2000).unwrap(); // Ignored bitmap overflow flag.
    assert_eq!(pixel(&m, 0, 0), 0x00ff00);
    assert_eq!(pixel(&m, 1, 0), 0);
    assert_eq!(pixel(&m, 2, 0), 0xff0000);
    transform(&mut m, 2, [-256, 0, 0, -256], [239 * 256, 159 * 256]);
    assert_eq!(pixel(&m, 0, 0), 0xffffff);
    assert_eq!(pixel(&m, 239, 159), 0);
    // Hardware reset is a zero matrix, not an implicit identity.
    transform(&mut m, 2, [0; 4], [256, 0]);
    assert!(frame(&m).pixels().iter().all(|&p| p == 0xff0000));
}

#[test]
fn transformed_backgrounds_composite_with_regular_and_affine_sprites() {
    for mode in [1, 2] {
        let mut m = setup(2, 1, true);
        m.write16(DISPCNT, mode | 0x1440).unwrap();
        for i in 0..128 {
            m.write16(OAM + i * 8, 0x200).unwrap();
        }
        m.write16(VRAM, 0x0101).unwrap();
        m.write16(VRAM + 0x10000, 0x2222).unwrap();
        m.write16(PAL + 0x204, 0x03e0).unwrap();
        m.write16(OAM, 0).unwrap();
        m.write16(OAM + 4, 0).unwrap();
        assert_eq!(pixel(&m, 0, 0), 0x00ff00); // OBJ wins equal BG priority.
        m.write16(OAM + 4, 0x400).unwrap();
        assert_eq!(pixel(&m, 0, 0), rgb555_to_rgb888(31));
        m.write16(OAM, 0x100).unwrap();
        m.write16(OAM + 6, 256).unwrap();
        m.write16(OAM + 30, 256).unwrap();
        m.write16(OAM + 4, 0).unwrap();
        assert_eq!(pixel(&m, 0, 0), 0x00ff00);
    }
}

#[test]
fn diagnostics_leave_output_unchanged_and_disabled_layers_skip_validation() {
    let mut m = setup(2, 3, false);
    let mut f = Framebuffer::default();
    f.clear(0x001f);
    for control in [0xc01, 0xd02, 0xe02] {
        m.write16(DISPCNT, control).unwrap();
        assert_eq!(
            m.render_frame(&mut f),
            Err(VideoError::UnsupportedLayers(control))
        );
    }
    m.write16(DISPCNT, 0x402).unwrap();
    m.write16(BG2CNT, 0xdf00).unwrap(); // Map31, 1024 pixels.
    m.write32(BG2Y, 128 * 256).unwrap(); // Flat row starts outside first64K.
    assert_eq!(
        m.render_frame(&mut f),
        Err(VideoError::UnsupportedMapAddress(0x10000))
    );
    assert!(f.pixels().iter().all(|&p| p == 0xff0000));
    m.write16(DISPCNT, 2).unwrap();
    m.render_frame(&mut f).unwrap();
    assert!(f.pixels().iter().all(|&p| p == rgb555_to_rgb888(0x4000)));
    m.write16(DISPCNT, 0xffff).unwrap();
    m.render_frame(&mut f).unwrap();
    assert!(f.pixels().iter().all(|&p| p == 0xffffff));
}

#[test]
fn cpu_demo_pans_rotates_zooms_clips_cancels_opposites_and_resets() {
    use gba_rust::{
        affine_demo::{AffineDemo, AFFINE_STATE},
        input::{Button, Buttons},
    };
    let mut demo = AffineDemo::new().unwrap();
    let mut f = Framebuffer::default();
    assert_eq!(demo.machine().memory().read16(DISPCNT).unwrap(), 0);
    assert_eq!(demo.machine().memory().read16(VRAM).unwrap(), 0);
    // Signed negative panning and opposite-key cancellation.
    demo.frame(
        Buttons::default()
            .with(Button::Left, true)
            .with(Button::Up, true),
        &mut f,
    )
    .unwrap();
    demo.frame(Buttons::from_bits(0xf0), &mut f).unwrap();
    assert_eq!(
        demo.machine().memory().read32(AFFINE_STATE + 4).unwrap() as i32,
        -2
    );
    assert_eq!(
        demo.machine().memory().read32(AFFINE_STATE + 8).unwrap() as i32,
        -2
    );
    for bits in 0..8 {
        let rotated = bits & 1 != 0;
        let zoomed = bits & 2 != 0;
        let clipped = bits & 4 != 0;
        let before = demo.machine().memory().display_position();
        demo.frame(
            Buttons::default()
                .with(Button::L, rotated)
                .with(Button::R, zoomed)
                .with(Button::A, clipped),
            &mut f,
        )
        .unwrap();
        let a = f64::from(match (rotated, zoomed) {
            (false, false) => 256,
            (false, true) => 128,
            (true, false) => 181,
            (true, true) => 90,
        }) / 256.0;
        let b = if rotated { a } else { 0.0 };
        for y in 0..160 {
            for x in 0..240 {
                let dx = x as f64 - 120.0;
                let dy = y as f64 - 80.0;
                let tx = (126.0 + a * dx + b * dy).floor() as i32;
                let ty = (126.0 - b * dx + a * dy).floor() as i32;
                let color = if clipped && (!(0..256).contains(&tx) || !(0..256).contains(&ty)) {
                    0x4000
                } else {
                    let tx = tx.rem_euclid(256) as usize;
                    let ty = ty.rem_euclid(256) as usize;
                    [
                        0x0260, 0x03a0, 0x7d20, 0x7e80, 0x001f, 0x421f, 0x03ff, 0x7fff,
                    ][((tx / 32 + 2 * (ty / 32)) % 4) * 2 + (tx / 2 + ty / 2) % 2]
                };
                assert_eq!(
                    f.pixels()[y * WIDTH + x],
                    rgb555_to_rgb888(color),
                    "input {bits}, pixel {x},{y}"
                );
            }
        }
        let m = demo.machine().memory();
        assert_eq!(m.read32(AFFINE_STATE).unwrap(), bits + 3);
        assert_eq!(m.read16(DISPCNT).unwrap(), 0x402);
        assert_eq!(m.display_position().vblanks, before.vblanks + 1);
        assert_eq!(m.display_position().scanline, 160);
    }
    demo.frame(
        Buttons::default()
            .with(Button::Start, true)
            .with(Button::Right, true),
        &mut f,
    )
    .unwrap();
    assert_eq!(demo.machine().memory().read32(AFFINE_STATE + 4).unwrap(), 0);
    assert_eq!(demo.machine().memory().read32(AFFINE_STATE + 8).unwrap(), 0);
}

#[test]
fn snapshot_uses_programmed_origin_without_advancing_or_accumulating_scanline_state() {
    let mut m = setup(2, 1, true);
    transform(&mut m, 2, [181, 181, -181, 181], [12345, -567]);
    let first = frame(&m);
    m.advance_cycles(1232 * 57);
    let cycles = m.cycles();
    assert_eq!(frame(&m).pixels(), first.pixels());
    assert_eq!(m.cycles(), cycles);
    m.write32(BG2X, 0).unwrap();
    assert_ne!(frame(&m).pixels(), first.pixels());
}
