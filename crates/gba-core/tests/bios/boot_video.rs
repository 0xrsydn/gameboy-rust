//! Original boot regressions. ROMs deliberately do not initialize affine matrices.
use super::*;
use gba_core::{
    io::{BG0CNT, DISPCNT},
    video::{rgb555_to_rgb888, Framebuffer, WIDTH},
};

fn started(control: u16) -> Machine {
    let mut machine = bios::boot(words(&[
        0xe3a0_1301, // MOV r1,#0x04000000
        0xe59f_0004, // LDR r0,[pc,#4]
        0xe1c1_00b0, // STRH r0,[r1] (DISPCNT only)
        0xeaff_fffe, // B .
        u32::from(control),
    ]))
    .unwrap();
    reach(&mut machine, ROM_START + 12, 100);
    machine
}

#[test]
fn boot_supplies_identity_for_bitmap_coordinates_without_rom_matrix_writes() {
    let mut machine = started(0x0403);
    for (x, y, color) in [
        (0, 0, 0x001f),
        (7, 0, 0x03e0),
        (0, 9, 0x7c00),
        (7, 9, 0x7fff),
    ] {
        machine
            .memory_mut()
            .write16(VRAM_START + 2 * (y * 240 + x), color)
            .unwrap();
    }
    let mut frame = Framebuffer::default();
    machine.memory().render_frame(&mut frame).unwrap();
    for (x, y, color) in [
        (0, 0, 0x001f),
        (7, 0, 0x03e0),
        (0, 9, 0x7c00),
        (7, 9, 0x7fff),
    ] {
        assert_eq!(frame.pixels()[y * WIDTH + x], rgb555_to_rgb888(color));
    }
    machine.memory_mut().set_scanline_rendering(true);
    for _ in 0..200000 {
        machine.step().unwrap();
        if machine.memory().captured_vblank() == Some(2) {
            break;
        }
    }
    assert_eq!(machine.memory().captured_vblank(), Some(2));
    assert!(machine.memory().present_frame(&mut frame).unwrap());
    assert_eq!(frame.pixels()[9 * WIDTH + 7], 0xffffff);
}

#[test]
fn boot_supplies_identity_for_both_affine_backgrounds() {
    for bg in [2, 3] {
        let mut machine = started(2 | (0x100 << bg));
        let bus = machine.memory_mut();
        bus.write16(BG0CNT + bg * 2, 0x1000).unwrap(); // map at VRAM+0x8000
        bus.write16(0x05000002, 0x001f).unwrap();
        bus.write16(0x05000004, 0x03e0).unwrap();
        bus.write16(0x05000006, 0x7c00).unwrap();
        bus.write16(VRAM_START, 0x0201).unwrap();
        bus.write16(VRAM_START + 8, 0x0003).unwrap();
        let mut frame = Framebuffer::default();
        bus.render_frame(&mut frame).unwrap();
        assert_eq!(frame.pixels()[0], 0xff0000, "BG{bg}");
        assert_eq!(frame.pixels()[1], 0x00ff00, "BG{bg}");
        assert_eq!(frame.pixels()[WIDTH], 0x0000ff, "BG{bg}");
    }
}

#[test]
fn constructing_memory_does_not_apply_firmware_defaults() {
    let mut bus = Memory::new(vec![]).unwrap();
    bus.write16(DISPCNT, 0x0403).unwrap();
    bus.write16(VRAM_START, 0x001f).unwrap();
    bus.write16(VRAM_START + 2, 0x03e0).unwrap();
    let mut frame = Framebuffer::default();
    bus.render_frame(&mut frame).unwrap();
    assert!(frame.pixels().iter().all(|pixel| *pixel == 0xff0000));
}
