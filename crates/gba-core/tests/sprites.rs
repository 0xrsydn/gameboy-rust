use gba_core::{
    io::{BG0CNT, BG2CNT, DISPCNT, GREENSWAP},
    memory::{Memory, MemoryError, OAM_START as OAM, PALETTE_START as PAL, VRAM_START as VRAM},
    video::{rgb555_to_rgb888, Framebuffer, VideoError, WIDTH},
};

#[path = "sprites/affine.rs"]
mod affine;

fn setup() -> Memory {
    let mut m = Memory::new(vec![]).unwrap();
    m.write16(DISPCNT, 0x1040).unwrap(); // OBJ, 1D, Mode 0
    for i in 0..128 {
        m.write16(OAM + i * 8, 0x200).unwrap();
    }
    for (i, color) in [0x4210, 0x001f, 0x03e0, 0x7c00, 0x7fff]
        .into_iter()
        .enumerate()
    {
        m.write16(PAL + 0x200 + i as u32 * 2, color).unwrap();
    }
    m.write16(PAL, 0x4210).unwrap();
    m
}

fn obj(m: &mut Memory, index: u32, a: u16, b: u16, c: u16) {
    m.write16(OAM + index * 8, a).unwrap();
    m.write16(OAM + index * 8 + 2, b).unwrap();
    m.write16(OAM + index * 8 + 4, c).unwrap();
}

fn fill(m: &mut Memory, slot: u32, bytes: u32, value: u16) {
    for offset in (0..bytes).step_by(2) {
        m.write16(VRAM + 0x10000 + ((slot * 32 + offset) & 0x7fff), value)
            .unwrap();
    }
}

fn render(m: &Memory) -> Framebuffer {
    let mut f = Framebuffer::default();
    m.render_frame(&mut f).unwrap();
    f
}

fn pixel(m: &Memory, x: usize, y: usize) -> u32 {
    render(m).pixels()[y * WIDTH + x]
}

#[test]
fn oam_mirrors_reads_all_bytes_and_ignores_only_byte_writes() {
    let mut m = setup();
    m.write32(OAM, 0xabcd_1234).unwrap();
    assert_eq!(m.read32(OAM + 0x400).unwrap(), 0xabcd_1234);
    assert_eq!(m.read8(OAM + 1).unwrap(), 0x12);
    m.write8(OAM, 0xff).unwrap();
    m.write8(OAM + 3, 0xff).unwrap();
    assert_eq!(m.read32(OAM).unwrap(), 0xabcd_1234);
    m.write32(0x07ff_fffc, 0x7654_3210).unwrap();
    assert_eq!(m.read32(OAM + 0x3fc).unwrap(), 0x7654_3210);
    m.write16(OAM + 6, 0xface).unwrap(); // Affine parameter slots are ordinary memory.
    assert_eq!(m.read16(OAM + 0x406).unwrap(), 0xface);
    assert_eq!(m.write16(OAM + 1, 0), Err(MemoryError::Unaligned(OAM + 1)));
    assert_eq!(m.write32(OAM + 2, 0), Err(MemoryError::Unaligned(OAM + 2)));
    assert_eq!(m.read32(OAM).unwrap(), 0xabcd_1234);
}

#[test]
fn four_bit_sprite_nibbles_banks_zero_transparency_and_opaque_black() {
    let mut m = setup();
    obj(&mut m, 0, 0, 0, 0xf000);
    fill(&mut m, 0, 2, 0x0321);
    m.write16(PAL + 0x200 + 241 * 2, 0x001f).unwrap();
    m.write16(PAL + 0x200 + 242 * 2, 0x03e0).unwrap();
    m.write16(PAL + 0x200 + 243 * 2, 0).unwrap();
    m.write16(PAL + 0x200 + 240 * 2, 0x7fff).unwrap();
    assert_eq!(
        &render(&m).pixels()[..4],
        &[0xff0000, 0x00ff00, 0, rgb555_to_rgb888(0x4210)]
    );
}

#[test]
fn eight_bit_sprite_ignores_bank_and_uses_obj_palette_not_bg_palette() {
    let mut m = setup();
    obj(&mut m, 0, 0x2000, 0, 0xf000);
    fill(&mut m, 0, 2, 0x00ff);
    m.write16(PAL + 0x3fe, 0x03e0).unwrap();
    m.write16(PAL + 0x1fe, 0x001f).unwrap();
    assert_eq!(pixel(&m, 0, 0), 0x00ff00);
    assert_eq!(pixel(&m, 1, 0), rgb555_to_rgb888(0x4210));
}

#[test]
fn every_shape_and_size_covers_exact_rectangle() {
    let sizes = [
        [(8, 8), (16, 16), (32, 32), (64, 64)],
        [(16, 8), (32, 8), (32, 16), (64, 32)],
        [(8, 16), (8, 32), (16, 32), (32, 64)],
    ];
    for (shape, dimensions) in sizes.iter().enumerate() {
        for (size, &(w, h)) in dimensions.iter().enumerate() {
            let mut m = setup();
            fill(&mut m, 0, 0x8000, 0x1111);
            obj(
                &mut m,
                0,
                ((shape as u16) << 14) | 5,
                ((size as u16) << 14) | 7,
                0,
            );
            let f = render(&m);
            for y in 0..160 {
                for x in 0..240 {
                    let expected = if (7..7 + w).contains(&x) && (5..5 + h).contains(&y) {
                        0xff0000
                    } else {
                        rgb555_to_rgb888(0x4210)
                    };
                    assert_eq!(
                        f.pixels()[y * WIDTH + x],
                        expected,
                        "shape {shape}, size {size}, pixel {x},{y}"
                    );
                }
            }
        }
    }
}

#[test]
fn tile_row_strides_differ_between_1d_and_2d_in_both_depths() {
    for eight in [false, true] {
        for one in [false, true] {
            let mut m = setup();
            m.write16(DISPCNT, if one { 0x1040 } else { 0x1000 })
                .unwrap();
            obj(&mut m, 0, if eight { 0x2000 } else { 0 }, 0x4000, 0);
            let slots = if eight { 2 } else { 1 };
            for (slot, index) in [
                (0, 1),
                (slots, 2),
                (if one { 2 * slots } else { 32 }, 3),
                (if one { 3 * slots } else { 32 + slots }, 4),
            ] {
                fill(
                    &mut m,
                    slot,
                    slots * 32,
                    if eight { index * 0x101 } else { index * 0x1111 },
                );
            }
            let f = render(&m);
            for (x, y, color) in [
                (0, 0, 0xff0000),
                (8, 0, 0x00ff00),
                (0, 8, 0x0000ff),
                (8, 8, 0xffffff),
            ] {
                assert_eq!(f.pixels()[y * WIDTH + x], color, "8bit={eight} 1D={one}");
            }
        }
    }
}

#[test]
fn eight_bit_odd_base_is_preserved_in_1d_but_aligned_in_2d() {
    let mut m = setup();
    fill(&mut m, 0, 32, 0x0101);
    fill(&mut m, 1, 32, 0x0202);
    obj(&mut m, 0, 0x2000, 0, 1);
    assert_eq!(pixel(&m, 0, 0), 0x00ff00);
    m.write16(DISPCNT, 0x1000).unwrap();
    assert_eq!(pixel(&m, 0, 0), 0xff0000);
}

#[test]
fn mapping_wraps_horizontal_2d_rows_and_the_32k_obj_area() {
    let mut m = setup();
    obj(&mut m, 0, 0x4000, 0, 1023); // 16x8
    fill(&mut m, 1023, 32, 0x1111);
    fill(&mut m, 0, 32, 0x2222);
    fill(&mut m, 992, 32, 0x3333);
    assert_eq!(pixel(&m, 8, 0), 0x00ff00); // 1D wraps entire area.
    m.write16(DISPCNT, 0x1000).unwrap();
    assert_eq!(pixel(&m, 8, 0), 0x0000ff); // 2D wraps within last row.
    obj(&mut m, 0, 0, 0x4000, 992); // 16x16
    assert_eq!(pixel(&m, 0, 8), 0x00ff00); // Next row wraps to first row.
}

#[test]
fn flips_reverse_whole_multi_tile_sprite_not_each_tile() {
    for eight in [false, true] {
        let mut m = setup();
        let slots = if eight { 2 } else { 1 };
        for i in 0..4 {
            fill(
                &mut m,
                i * slots,
                32 * slots,
                if eight {
                    (i as u16 + 1) * 0x101
                } else {
                    (i as u16 + 1) * 0x1111
                },
            );
        }
        for (flips, color) in [
            (0, 0xff0000),
            (0x1000, 0x00ff00),
            (0x2000, 0x0000ff),
            (0x3000, 0xffffff),
        ] {
            obj(&mut m, 0, if eight { 0x2000 } else { 0 }, 0x4000 | flips, 0);
            assert_eq!(pixel(&m, 0, 0), color);
        }
    }
}

#[test]
fn screen_coordinates_wrap_and_clip_at_each_edge() {
    let mut m = setup();
    fill(&mut m, 0, 32, 0x1111);
    for (ox, oy) in [(508, 252), (238, 158), (250, 170)] {
        obj(&mut m, 0, oy, ox, 0);
        let f = render(&m);
        for y in 0..160 {
            for x in 0..240 {
                let inside =
                    ((x + 512 - ox as usize) & 511) < 8 && ((y + 256 - oy as usize) & 255) < 8;
                assert_eq!(
                    f.pixels()[y * WIDTH + x],
                    if inside {
                        0xff0000
                    } else {
                        rgb555_to_rgb888(0x4210)
                    }
                );
            }
        }
    }
}

#[test]
fn object_priority_wins_then_oam_index_and_transparent_pixels_expose_later_objs() {
    let mut m = setup();
    fill(&mut m, 0, 32, 0x1111);
    fill(&mut m, 1, 32, 0x2222);
    obj(&mut m, 0, 0, 0, 0xc00); // First OBJ has lowest BG priority.
    obj(&mut m, 127, 0, 0, 1); // Later OBJ has highest BG priority.
    assert_eq!(pixel(&m, 0, 0), 0x00ff00);
    obj(&mut m, 127, 0, 0, 0xc01); // Equal priority: first OAM entry wins.
    assert_eq!(pixel(&m, 0, 0), 0xff0000);
    // BG0 priority2 covers both priority3 objects.
    m.write16(DISPCNT, 0x1140).unwrap();
    m.write16(BG0CNT, 0x1002).unwrap();
    m.write16(VRAM, 0x3333).unwrap();
    m.write16(PAL + 6, 0x7c00).unwrap();
    assert_eq!(pixel(&m, 0, 0), 0x0000ff);
    obj(&mut m, 0, 0, 0, 0);
    obj(&mut m, 127, 0, 0, 1);
    fill(&mut m, 0, 2, 0x0010); // Zero only at first pixel.
    assert_eq!(pixel(&m, 0, 0), 0x00ff00);
    assert_eq!(pixel(&m, 1, 0), 0xff0000);
}

#[test]
fn sprite_beats_bg_on_equal_priority_but_not_higher_priority_bg() {
    let mut m = setup();
    obj(&mut m, 0, 0, 0, 0x400);
    fill(&mut m, 0, 32, 0x1111);
    m.write16(DISPCNT, 0x1140).unwrap();
    m.write16(BG0CNT, 0x1001).unwrap();
    m.write16(VRAM, 0x2222).unwrap();
    m.write16(PAL + 4, 0x03e0).unwrap();
    assert_eq!(pixel(&m, 0, 0), 0xff0000);
    m.write16(BG0CNT, 0x1000).unwrap();
    assert_eq!(pixel(&m, 0, 0), 0x00ff00);
    m.write16(VRAM, 0).unwrap();
    assert_eq!(pixel(&m, 0, 0), 0xff0000); // Transparent BG reveals sprite.
}

#[test]
fn mode3_uses_bg2_priority_and_only_upper_obj_tile_numbers() {
    let mut m = setup();
    m.write16(DISPCNT, 0x1443).unwrap();
    m.write16(VRAM, 0x03e0).unwrap();
    fill(&mut m, 0, 32, 0x1111);
    fill(&mut m, 512, 32, 0x1111);
    obj(&mut m, 0, 0, 0, 0);
    assert_eq!(pixel(&m, 0, 0), 0x00ff00);
    obj(&mut m, 0, 0, 0, 512);
    assert_eq!(pixel(&m, 0, 0), 0xff0000);
    obj(&mut m, 0, 0, 0, 512 | 0x400);
    assert_eq!(pixel(&m, 0, 0), 0x00ff00);
    m.write16(BG2CNT, 1).unwrap();
    assert_eq!(pixel(&m, 0, 0), 0xff0000);
    m.write16(DISPCNT, 0x1043).unwrap();
    assert_eq!(pixel(&m, 0, 0), 0xff0000); // BG2 disabled.
}

#[test]
fn disabled_objects_ignore_unsupported_bits_and_global_disable_skips_all_objects() {
    let mut m = setup();
    obj(&mut m, 0, 0xfe00, 0xffff, 0xffff); // Disabled, despite mode/mosaic/shape bits.
    assert_eq!(pixel(&m, 0, 0), rgb555_to_rgb888(0x4210));
    obj(&mut m, 0, 0xc00, 0, 0); // Prohibited OBJ mode.
    m.write16(DISPCNT, 0).unwrap();
    assert_eq!(pixel(&m, 0, 0), rgb555_to_rgb888(0x4210));
}

#[test]
fn unsupported_objects_leave_output_unchanged_and_forced_blank_bypasses_errors() {
    let mut m = setup();
    fill(&mut m, 0, 32, 0x2222);
    obj(&mut m, 0, 0, 0, 0);
    let mut frame = Framebuffer::default();
    frame.clear(0x001f);
    for attr in [0xc00, 0xc000, 0xd00, 0x1c00, 0xc300] {
        obj(&mut m, 127, attr, 0, 0);
        assert!(matches!(
            m.render_frame(&mut frame),
            Err(VideoError::UnsupportedObject { index: 127, .. })
        ));
        assert!(frame.pixels().iter().all(|&p| p == 0xff0000));
    }
    m.write16(DISPCNT, 0x1080).unwrap();
    m.render_frame(&mut frame).unwrap();
    assert!(frame.pixels().iter().all(|&p| p == 0xffffff));
}

#[test]
fn green_swap_applies_after_sprite_composition() {
    let mut m = setup();
    obj(&mut m, 0, 0, 0, 0);
    fill(&mut m, 0, 2, 0x21);
    m.write16(GREENSWAP, 1).unwrap();
    assert_eq!(&render(&m).pixels()[..2], &[0xffff00, 0]);
}

#[test]
fn oam_bus_uses_one_cycle_for_every_width() {
    use gba_core::timing::{bus_cycles, AccessKind, AccessWidth};
    for kind in [AccessKind::Sequential, AccessKind::NonSequential] {
        for width in [AccessWidth::Byte, AccessWidth::Halfword, AccessWidth::Word] {
            assert_eq!(bus_cycles(0, OAM, width, kind), 1);
        }
    }
}

#[test]
fn dma_copies_words_to_oam_and_reads_them_back() {
    use gba_core::{
        cpu::Cpu,
        dma::DMA_BASE,
        machine::{Machine, StepKind},
    };
    let mut machine = Machine::new(Cpu::new(0x0300_0000), setup());
    machine
        .memory_mut()
        .write32(0x0200_0000, 0x1234_abcd)
        .unwrap();
    for (source, dest) in [(0x0200_0000, OAM + 0x400), (OAM, 0x0200_0004)] {
        let m = machine.memory_mut();
        m.write32(DMA_BASE + 36, source).unwrap();
        m.write32(DMA_BASE + 40, dest).unwrap();
        m.write32(DMA_BASE + 44, 0x8400_0001).unwrap();
        assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 3 });
    }
    assert_eq!(machine.memory().read32(0x0200_0004).unwrap(), 0x1234_abcd);
}
