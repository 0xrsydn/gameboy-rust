use gba_core::{
    cpu::Cpu,
    display::{CYCLES_PER_FRAME, CYCLES_PER_LINE, HBLANK_START, VBLANK_START},
    dma::DMA_BASE,
    input::{Button, Buttons},
    io::*,
    machine::{Machine, StepKind},
    memory::{Memory, OAM_START as OAM, PALETTE_START as PAL, ROM_START, VRAM_START as VRAM},
    video::{rgb555_to_rgb888 as rgb, Framebuffer, VideoError, HEIGHT, WIDTH},
};
use gba_demos::raster_demo::{RasterDemo, RASTER_STATE, RASTER_TABLE};

#[path = "scanlines/capture.rs"]
mod capture;
#[path = "scanlines/machine.rs"]
mod machine;
#[path = "scanlines/rendering.rs"]
mod rendering;

fn memory() -> Memory {
    let mut m = Memory::new(vec![]).unwrap();
    m.set_scanline_rendering(true);
    m
}

fn present(m: &Memory) -> Framebuffer {
    let mut f = Framebuffer::default();
    assert!(m.present_frame(&mut f).unwrap());
    f
}

fn at(f: &Framebuffer, x: usize, y: usize) -> u32 {
    f.pixels()[y * WIDTH + x]
}

fn word_rom(code: &[u32]) -> Vec<u8> {
    code.iter().flat_map(|v| v.to_le_bytes()).collect()
}
