use super::*;
use gba_rust::{
    input::{Button, Buttons},
    mosaic_demo::{MosaicDemo, MOSAIC_STATE},
    tile_demo::TileDemo,
};

#[test]
fn cpu_demo_cycles_all_sizes_and_wraps_without_host_video_writes() {
    let mut demo = MosaicDemo::new().unwrap();
    let mut frame = Framebuffer::default();
    assert_eq!(demo.machine().memory().read16(DISPCNT).unwrap(), 0);
    assert_eq!(demo.machine().memory().read16(VRAM).unwrap(), 0);
    for update in 1..=129 {
        demo.frame(Buttons::default(), &mut frame).unwrap();
        let m = demo.machine().memory();
        assert_eq!(m.read32(MOSAIC_STATE).unwrap(), update);
        assert_eq!(m.read16(DISPCNT).unwrap(), 0x1340);
        assert_eq!(m.read16(BG0CNT).unwrap(), 0x1844);
        assert_eq!(m.read16(BG1CNT).unwrap(), 0xd0c1);
        assert_eq!(m.read16(OAM).unwrap(), 0x1048);
        assert_eq!(m.read32(MOSAIC).unwrap(), 0);
        assert_eq!(m.display_position().scanline, 160);
        let size = (1 + (update / 8) % 16) as usize;
        // Check terrain and foreground away from the sprite for every size.
        for y in 0..48 {
            for x in 0..WIDTH {
                let sx = x / size * size;
                let sy = y / size * size;
                let cross = sx / 8 % 4 == 1 && sy / 8 % 4 == 1 && (sx % 8 == 3 || sy % 8 == 3);
                let color = if cross {
                    0x3ff
                } else {
                    [0x260, 0x3a0, 0x7d20, 0x7e80]
                        [((sx / 32 + sy / 32) % 2) * 2 + (sx / 2 + sy / 2) % 2]
                };
                assert_eq!(at(&frame, x, y), rgb(color), "update={update} at={x},{y}");
            }
        }
    }
}

#[test]
fn both_bypasses_restore_original_scene_with_scroll_rotation_zoom_and_reset() {
    let mut demo = MosaicDemo::new().unwrap();
    let mut base = TileDemo::new().unwrap();
    let mut f = Framebuffer::default();
    let mut reference = Framebuffer::default();
    for i in 0..32 {
        let buttons = Buttons::default()
            .with(Button::Left, i < 16)
            .with(Button::Down, true)
            .with(Button::Start, i == 31)
            .with(Button::L, (8..24).contains(&i))
            .with(Button::R, (16..28).contains(&i));
        base.frame(buttons, &mut reference).unwrap();
        demo.frame(buttons.with(Button::A, true).with(Button::B, true), &mut f)
            .unwrap();
        assert_eq!(f.pixels(), reference.pixels());
        let m = demo.machine().memory();
        assert_ne!(m.read16(OAM).unwrap() & 0x1000, 0);
        assert_eq!(m.read16(OAM + 2).unwrap() & 0x1000, 0); // Z no longer flips.
        assert_eq!(m.read16(OAM + 4).unwrap() & 0xc00, 0); // X no longer lowers priority.
        assert_eq!(
            m.read32(MOSAIC_STATE + 4).unwrap(),
            base.machine().memory().read32(MOSAIC_STATE + 4).unwrap()
        );
    }
    assert_eq!(demo.machine().memory().read32(MOSAIC_STATE + 4).unwrap(), 0);
    assert_eq!(demo.machine().memory().read32(MOSAIC_STATE + 8).unwrap(), 0);
}

#[test]
fn dma_word_write_sets_bg_and_obj_fields_while_unused_halfword_ignores_writes() {
    use gba_rust::{
        cpu::Cpu,
        dma::DMA_BASE,
        machine::{Machine, StepKind},
    };
    let mut m = memory();
    m.write16(DISPCNT, 0x1140).unwrap();
    m.write16(BG0CNT, 0x1040).unwrap();
    fill(&mut m, VRAM, 32, 0x1212);
    m.write16(PAL + 4, 31).unwrap();
    m.write16(PAL + 2, 0x3e0).unwrap();
    object(&mut m, 0, 0x1000, 0, 0);
    fill(&mut m, VRAM + 0x10000, 32, 0x0101);
    m.write16(PAL + 0x202, 0x7c00).unwrap();
    m.write32(0x0200_0000, 0xffff_0302).unwrap(); // BG3x1, OBJ4x1.
    m.write32(DMA_BASE + 36, 0x0200_0000).unwrap();
    m.write32(DMA_BASE + 40, MOSAIC).unwrap();
    m.write32(DMA_BASE + 44, 0x8400_0001).unwrap();
    let mut machine = Machine::new(Cpu::new(0x0300_0000), m);
    assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 3 });
    let f = render(machine.memory());
    assert_eq!(at(&f, 3, 0), rgb(0x7c00));
    assert_eq!(at(&f, 8, 0), rgb(31)); // BG samples x6, not x8's independent phase.
    assert_eq!(at(&f, 9, 0), rgb(0x3e0));
    assert_eq!(machine.memory().read32(MOSAIC).unwrap(), 0);
}
