use gba_rust::{
    io::*,
    memory::{Memory, MemoryError, OAM_START as OAM, PALETTE_START as PAL, VRAM_START as VRAM},
    video::{rgb555_to_rgb888 as rgb, Framebuffer, HEIGHT, WIDTH},
};

#[path = "effects/colors.rs"]
mod colors;
#[path = "effects/demo.rs"]
mod demo;
#[path = "effects/windows.rs"]
mod windows;

fn setup() -> Memory {
    let mut m = Memory::new(vec![]).unwrap();
    m.write16(DISPCNT, 0x700).unwrap();
    m.write16(PAL, 0x4210).unwrap();
    for (bg, color) in [0x001f, 0x03e0, 0x7c00].into_iter().enumerate() {
        let bg = bg as u32;
        m.write16(PAL + (bg + 1) * 2, color).unwrap();
        m.write16(BG0CNT + bg * 2, (((24 + bg) << 8) | (bg << 2) | bg) as u16)
            .unwrap();
        for offset in (0..32).step_by(2) {
            m.write16(VRAM + bg * 0x4000 + offset, (bg as u16 + 1) * 0x1111)
                .unwrap();
        }
    }
    for obj in 0..128 {
        m.write16(OAM + obj * 8, 0x200).unwrap();
    }
    for offset in (0..32).step_by(2) {
        m.write16(VRAM + 0x10000 + offset, 0x1111).unwrap();
    }
    m.write16(PAL + 0x202, 0x7fff).unwrap();
    m
}

fn render(m: &Memory) -> Framebuffer {
    let mut f = Framebuffer::default();
    m.render_frame(&mut f).unwrap();
    f
}

fn pixel(m: &Memory, x: usize, y: usize) -> u32 {
    render(m).pixels()[y * WIDTH + x]
}

fn object(m: &mut Memory, index: u32, a: u16, b: u16, c: u16) {
    m.write16(OAM + index * 8, a).unwrap();
    m.write16(OAM + index * 8 + 2, b).unwrap();
    m.write16(OAM + index * 8 + 4, c).unwrap();
}

fn window(m: &mut Memory, h: u16, v: u16, inside: u16, outside: u16) {
    m.write16(DISPCNT, m.read16(DISPCNT).unwrap() | 0x2000)
        .unwrap();
    m.write16(WIN0H, h).unwrap();
    m.write16(WIN0V, v).unwrap();
    m.write16(WININ, inside).unwrap();
    m.write16(WINOUT, outside).unwrap();
}
