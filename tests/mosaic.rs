use gba_rust::{
    io::*,
    memory::{Memory, MemoryError, OAM_START as OAM, PALETTE_START as PAL, VRAM_START as VRAM},
    video::{rgb555_to_rgb888 as rgb, Framebuffer, HEIGHT, WIDTH},
};

#[path = "mosaic/backgrounds.rs"]
mod backgrounds;
#[path = "mosaic/demo.rs"]
mod demo;
#[path = "mosaic/objects.rs"]
mod objects;

fn memory() -> Memory {
    let mut m = Memory::new(vec![]).unwrap();
    for index in 0..128 {
        m.write16(OAM + index * 8, 0x200).unwrap();
    }
    for index in 0..256 {
        let color = (index & 31) | (((index >> 3) & 31) << 5) | (((index * 7) & 31) << 10);
        m.write16(PAL + index * 2, color as u16).unwrap();
        m.write16(PAL + 0x200 + index * 2, color as u16).unwrap();
    }
    m
}

fn render(m: &Memory) -> Framebuffer {
    let mut f = Framebuffer::default();
    m.render_frame(&mut f).unwrap();
    f
}

fn object(m: &mut Memory, index: u32, a: u16, b: u16, c: u16) {
    m.write16(OAM + index * 8, a).unwrap();
    m.write16(OAM + index * 8 + 2, b).unwrap();
    m.write16(OAM + index * 8 + 4, c).unwrap();
}

fn fill(m: &mut Memory, address: u32, bytes: u32, value: u16) {
    for offset in (0..bytes).step_by(2) {
        m.write16(address + offset, value).unwrap();
    }
}

fn at(frame: &Framebuffer, x: usize, y: usize) -> u32 {
    frame.pixels()[y * WIDTH + x]
}
