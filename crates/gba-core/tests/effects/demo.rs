use super::*;
use gba_core::input::{Button, Buttons};
use gba_demos::{
    effects_demo::{EffectsDemo, EFFECTS_STATE},
    tile_demo::TileDemo,
};

#[test]
fn cpu_demo_moves_wraps_cancels_resets_and_updates_effect_registers_during_vblank() {
    let mut demo = EffectsDemo::new().unwrap();
    let mut frame = Framebuffer::default();
    assert_eq!(demo.machine().memory().read16(DISPCNT).unwrap(), 0);
    let mut expected_x = 0u32;
    let mut expected_y = 0u32;
    let sequence = [
        Buttons::default(),
        Buttons::default()
            .with(Button::Left, true)
            .with(Button::Up, true),
        Buttons::default()
            .with(Button::Right, true)
            .with(Button::Down, true),
        Buttons::default()
            .with(Button::Right, true)
            .with(Button::Left, true),
        Buttons::default().with(Button::A, true),
        Buttons::default().with(Button::B, true),
        Buttons::default()
            .with(Button::A, true)
            .with(Button::B, true),
        Buttons::default()
            .with(Button::Start, true)
            .with(Button::Right, true),
        Buttons::default(),
    ];
    for (i, buttons) in sequence.into_iter().enumerate() {
        let before = demo.machine().cycles();
        demo.frame(buttons, &mut frame).unwrap();
        let memory = demo.machine().memory();
        // Explicit expected movements keep this independent of the ARM program.
        if i == 1 {
            expected_x = 510;
            expected_y = 510;
        }
        if i == 2 || i == 7 {
            expected_x = 0;
            expected_y = 0;
        }
        assert_eq!(memory.read32(EFFECTS_STATE).unwrap(), i as u32 + 1);
        assert_eq!(memory.read32(EFFECTS_STATE + 4).unwrap(), expected_x);
        assert_eq!(memory.read32(EFFECTS_STATE + 8).unwrap(), expected_y);
        assert_eq!(memory.read16(DISPCNT).unwrap(), 0x3340);
        assert_eq!(memory.read16(WININ).unwrap(), 0x1f);
        assert_eq!(memory.read16(WINOUT).unwrap(), 0x3f);
        assert_eq!(memory.read16(BLDALPHA).unwrap(), 0x808);
        assert_eq!(
            memory.read16(BLDCNT).unwrap(),
            match i {
                4 | 6 => 0x251,
                5 => 0xff,
                _ => 0xbf,
            }
        );
        assert_eq!(memory.display_position().scanline, 160);
        assert!(demo.machine().cycles() > before);
        // The center is in WIN0 and therefore unchanged by any effect.
        assert_eq!(frame.pixels()[80 * WIDTH + 120], rgb(0x7fff));
    }
}

#[test]
fn cpu_brightness_demo_matches_tile_scene_inside_and_outside_moving_window() {
    let mut demo = EffectsDemo::new().unwrap();
    let mut base = TileDemo::new().unwrap();
    let mut actual = Framebuffer::default();
    let mut original = Framebuffer::default();
    // Include wrapped scroll, affine objects, and brightness changes.
    for (index, buttons) in [
        Buttons::default()
            .with(Button::Left, true)
            .with(Button::Up, true),
        Buttons::default()
            .with(Button::B, true)
            .with(Button::L, true),
        Buttons::default()
            .with(Button::B, true)
            .with(Button::R, true),
        Buttons::default().with(Button::Start, true),
    ]
    .into_iter()
    .enumerate()
    {
        demo.frame(buttons, &mut actual).unwrap();
        base.frame(buttons, &mut original).unwrap();
        let (left, top) = if index < 3 { (62, 38) } else { (64, 40) };
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let old = original.pixels()[y * WIDTH + x];
                // Recover the exact five-bit channels from RGB888.
                let channels = [(old >> 19) & 31, (old >> 11) & 31, (old >> 3) & 31];
                let mut expected = 0u16;
                for (channel, shift) in channels.into_iter().zip([0, 5, 10]) {
                    let changed = if (left..left + 112).contains(&x) && (top..top + 80).contains(&y)
                    {
                        channel
                    } else if index == 1 || index == 2 {
                        channel - channel / 2
                    } else {
                        channel + (31 - channel) / 2
                    };
                    expected |= (changed as u16) << shift;
                }
                assert_eq!(
                    actual.pixels()[y * WIDTH + x],
                    rgb(expected),
                    "frame={index} pixel={x},{y}"
                );
            }
        }
    }
}

#[test]
fn cpu_window_demo_retains_active_region_when_bottom_moves_beyond_vcount_range() {
    let mut demo = EffectsDemo::new().unwrap();
    let mut frame = Framebuffer::default();
    for index in 0..54 {
        demo.frame(Buttons::default().with(Button::Down, true), &mut frame)
            .unwrap();
        // x=80 avoids foreground crosses and sprites. It stays inside WIN0's X bounds.
        let sy = 2 * (index + 1);
        let raw: u16 =
            [0x260, 0x3a0, 0x7d20, 0x7e80][((80 / 32 + sy / 32) % 2) * 2 + (80 / 2 + sy / 2) % 2];
        let expected = if index == 53 {
            // New bottom=228 cannot match any line. Last frame's active flag survives.
            raw
        } else {
            [0, 5, 10].into_iter().fold(0, |value, shift| {
                let channel = (raw >> shift) & 31;
                value | ((channel + (31 - channel) / 2) << shift)
            })
        };
        assert_eq!(frame.pixels()[80], rgb(expected), "frame={index}");
    }
    let mut snapshot = Framebuffer::default();
    demo.machine().memory().render_frame(&mut snapshot).unwrap();
    assert_ne!(snapshot.pixels()[80], frame.pixels()[80]);
}

#[test]
fn dma_word_write_updates_selection_and_coefficients_together() {
    use gba_core::{
        cpu::Cpu,
        dma::DMA_BASE,
        machine::{Machine, StepKind},
    };
    let mut m = setup();
    m.write32(0x0200_0000, 0x0808_0241).unwrap();
    m.write32(DMA_BASE + 36, 0x0200_0000).unwrap();
    m.write32(DMA_BASE + 40, BLDCNT).unwrap();
    m.write32(DMA_BASE + 44, 0x8400_0001).unwrap();
    let mut machine = Machine::new(Cpu::new(0x0300_0000), m);
    assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 3 });
    assert_eq!(machine.memory().read32(BLDCNT).unwrap(), 0x0808_0241);
    assert_eq!(pixel(machine.memory(), 0, 0), rgb(0x1ef));
}
