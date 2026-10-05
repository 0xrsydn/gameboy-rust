//! Original ARM Mode 2 program. The CPU copies assets through BIOS services
//! and writes affine coefficients/reference points during VBlank.
//! The bitmap demos reuse this instruction builder with different assets and page control.

use crate::{
    graphics_demo::{GraphicsDemo, GraphicsError},
    input::Buttons,
    machine::Machine,
    memory::{MemoryError, ROM_START},
    video::Framebuffer,
};

/// Debug words: completed updates, signed pan X, signed pan Y.
pub const AFFINE_STATE: u32 = 0x0200_0000;

pub struct AffineDemo {
    runner: GraphicsDemo,
}

impl AffineDemo {
    pub fn new() -> Result<Self, MemoryError> {
        Ok(Self {
            runner: GraphicsDemo::with_program(program(2, false))?,
        })
    }
    pub fn machine(&self) -> &Machine {
        self.runner.machine()
    }
    pub fn frame(
        &mut self,
        buttons: Buttons,
        output: &mut Framebuffer,
    ) -> Result<usize, GraphicsError> {
        self.runner.frame_with_control(buttons, output, 0x402)
    }
}

pub(crate) fn bitmap_program(mode: u16) -> Vec<u8> {
    assert!(matches!(mode, 4 | 5));
    program(mode, false)
}

pub(crate) fn raster_program() -> Vec<u8> {
    program(2, true)
}

fn program(mode: u16, raster: bool) -> Vec<u8> {
    let bitmap = mode != 2;
    let mut code = vec![
        0xe3a0_9301,                                    // MOV r9,#0x04000000
        0xe3a0_0080,                                    // MOV r0,#128 (forced blank)
        0xe1c9_00b0,                                    // STRH r0,[r9]
        0,                                              // LDR r0,=palette
        0xe3a0_1405,                                    // MOV r1,#0x05000000
        0xe3a0_2c01,                                    // MOV r2,#256 halfwords
        0xef0b_0000,                                    // CpuSet
        0,                                              // LDR r0,=VRAM image
        0xe3a0_1406,                                    // MOV r1,#0x06000000
        if bitmap { 0xe3a0_2a05 } else { 0xe3a0_2901 }, // MOV r2,#20480 or #16384 words
        0xef0c_0000,                                    // CpuFastSet
        0,                                              // LDR r0,=IRQ callback
        0xe3a0_1403,                                    // MOV r1,#0x03000000
        0xe281_1c7f,                                    // ADD r1,r1,#0x7f00
        0xe581_00fc,                                    // STR r0,[r1,#252]
        0xe3a0_0008,                                    // MOV r0,#8
        0xe1c9_00b4,                                    // STRH r0,[r9,#4] (VBlank request)
        0xe289_1c02,                                    // ADD r1,r9,#0x200
        0xe3a0_0001,                                    // MOV r0,#1
        0xe1c1_00b0,                                    // STRH r0,[r1] (IE)
        0xe289_ac01,                                    // ADD r10,r9,#0x100
        0xe28a_a030,                                    // ADD r10,r10,#0x30 (KEYINPUT)
        0xe289_6020,                                    // ADD r6,r9,#0x20 (BG2 affine registers)
        0xe3a0_b402,                                    // MOV r11,#0x02000000
        0xe3a0_4000,                                    // MOV r4,#0 (signed pan X)
        0xe3a0_5000,                                    // MOV r5,#0 (signed pan Y)
        0xe3a0_7000,                                    // MOV r7,#0 (updates)
        0xe3a0_0b01,                                    // MOV r0,#0x400
        0xe380_0000 | u32::from(mode),                  // ORR r0,r0,#mode (BG2 enabled)
        0xe1c9_00b0,                                    // STRH r0,[r9]
    ];
    let frame = code.len();
    code.extend([
        0xef05_0000,                                    // VBlankIntrWait
        0xe1da_30b0,                                    // LDRH r3,[r10]
        0xe313_0010,                                    // TST r3,#Right
        0x0284_4002,                                    // ADDEQ r4,r4,#2
        0xe313_0020,                                    // TST r3,#Left
        0x0244_4002,                                    // SUBEQ r4,r4,#2
        0xe313_0040,                                    // TST r3,#Up
        0x0245_5002,                                    // SUBEQ r5,r5,#2
        0xe313_0080,                                    // TST r3,#Down
        0x0285_5002,                                    // ADDEQ r5,r5,#2
        0xe313_0008,                                    // TST r3,#Start
        0x03a0_4000,                                    // MOVEQ r4,#0
        0x03a0_5000,                                    // MOVEQ r5,#0
        if bitmap { 0xe3a0_0000 } else { 0xe3a0_0a07 }, // MOV r0,#0 or #0x7000 (tile wrap)
        0xe313_0001, // TST r3,#A (Z: disable tile wrapping; bitmap page handled below)
        if raster { 0xe1a0_0000 } else { 0x03c0_0a02 }, // Raster Z bypasses distortion, not wrapping.
        0xe1c9_00bc,                                    // STRH r0,[r9,#12] (BG2CNT)
        0xe3a0_0c01,                                    // MOV r0,#256 (PA)
        0xe3a0_1000,                                    // MOV r1,#0 (PB)
        0xe3a0_cc01,                                    // MOV r12,#256 (PD)
        0xe313_0c02,                                    // TST r3,#L (Q: 45-degree rotation)
        0x03a0_00b5,                                    // MOVEQ r0,#181
        0x03a0_10b5,                                    // MOVEQ r1,#181
        0x03a0_c0b5,                                    // MOVEQ r12,#181
        0xe313_0c01,                                    // TST r3,#R (W: 2x zoom)
        0x01a0_00a0,                                    // MOVEQ r0,r0,LSR #1
        0x01a0_10a1,                                    // MOVEQ r1,r1,LSR #1
        0x01a0_c0ac,                                    // MOVEQ r12,r12,LSR #1
        0xe261_2000,                                    // RSB r2,r1,#0 (PC=-PB)
        0xe1c6_00b0,                                    // STRH r0,[r6] (PA)
        0xe1c6_10b2,                                    // STRH r1,[r6,#2] (PB)
        0xe1c6_20b4,                                    // STRH r2,[r6,#4] (PC)
        0xe1c6_c0b6,                                    // STRH r12,[r6,#6] (PD)
        // Reference = (map center + pan)*256 - matrix*(screen center).
        0xe3a0_8078, // MOV r8,#120
        0xe000_0098, // MUL r0,r8,r0 (120*PA)
        0xe3a0_8050, // MOV r8,#80
        0xe020_0198, // MLA r0,r8,r1,r0 (+80*PB)
        0xe260_0000, // RSB r0,r0,#0
        0xe080_0404, // ADD r0,r0,r4,LSL #8
        match mode {
            4 => 0xe280_0c78,
            5 => 0xe280_0a05,
            _ => 0xe280_0902,
        }, // ADD map center X * 256
        0xe586_0008, // STR r0,[r6,#8] (BG2X)
        0xe3a0_8078, // MOV r8,#120
        0xe002_0298, // MUL r2,r8,r2 (120*PC)
        0xe3a0_8050, // MOV r8,#80
        0xe022_2c98, // MLA r2,r8,r12,r2 (+80*PD)
        0xe262_2000, // RSB r2,r2,#0
        0xe082_2405, // ADD r2,r2,r5,LSL #8
        match mode {
            4 => 0xe282_2c50,
            5 => 0xe282_2901,
            _ => 0xe282_2902,
        }, // ADD map center Y * 256
        0xe586_200c, // STR r2,[r6,#12] (BG2Y)
        0xe287_7001, // ADD r7,r7,#1
    ]);
    if bitmap {
        code.extend([
            0xe3a0_0b01,                   // MOV r0,#0x400
            0xe380_0000 | u32::from(mode), // ORR r0,r0,#mode
            0xe317_0020,                   // TST r7,#32 (alternate pages every32 updates)
            0x1380_0010,                   // ORRNE r0,r0,#16
            0xe313_0001,                   // TST r3,#A (Z: force page1)
            0x0380_0010,                   // ORREQ r0,r0,#16
            0xe1c9_00b0,                   // STRH r0,[r9] (page switch during VBlank)
        ]);
    }
    let mut raster_dma_load = None;
    if raster {
        code.extend([
            0xe3a0_2402, // MOV r2,#0x02000000
            0xe282_2a01, // ADD r2,r2,#0x1000 (PB table)
            0xe3a0_0000, // MOV r0,#0 (table row)
        ]);
        let row = code.len();
        code.extend([
            0xe310_0010, // TST r0,#16
            0x0281_8c02, // ADDEQ r8,r1,#512 (base PB + two pixels)
            0x1241_8c02, // SUBNE r8,r1,#512 (base PB - two pixels)
            0xe313_0001, // TST r3,#A (Z: bypass distortion)
            0x01a0_8001, // MOVEQ r8,r1
            0xe0c2_80b2, // STRH r8,[r2],#2
            0xe280_0001, // ADD r0,r0,#1
            0xe350_00a0, // CMP r0,#160
        ]);
        let displacement = row as i32 - code.len() as i32 - 2;
        code.push(0x1a00_0000 | (displacement as u32 & 0x00ff_ffff));
        code.extend([
            0xe3a0_0000, // MOV r0,#0
            0xe1c9_0bba, // STRH r0,[r9,#0xba] (disable DMA0)
            0xe3a0_0402, // MOV r0,#0x02000000
            0xe280_0a01, // ADD r0,r0,#0x1000
            0xe589_00b0, // STR r0,[r9,#0xb0] (source)
            0xe289_0022, // ADD r0,r9,#0x22 (BG2PB)
            0xe589_00b4, // STR r0,[r9,#0xb4] (fixed destination)
        ]);
        raster_dma_load = Some(code.len());
        code.push(0); // LDR r0,=DMA repeat/HBlank configuration
        code.push(0xe589_00b8); // STR r0,[r9,#0xb8] (arm DMA0)
    }
    code.extend([
        0xe58b_7000, // STR r7,[r11]
        0xe58b_4004, // STR r4,[r11,#4]
        0xe58b_5008, // STR r5,[r11,#8]
    ]);
    let offset = frame as i32 - code.len() as i32 - 2;
    code.push(0xea00_0000 | (offset as u32 & 0xffffff));
    let irq = ROM_START + code.len() as u32 * 4;
    code.extend([
        0xe280_0c02, // ADD r0,r0,#0x200 (BIOS supplies I/O base)
        0xe1d0_10b2, // LDRH r1,[r0,#2]
        0xe1c0_10b2, // STRH r1,[r0,#2] (ack IF)
        0xe3a0_0403, // MOV r0,#0x03000000
        0xe280_0c7f, // ADD r0,r0,#0x7f00
        0xe1d0_2fb8, // LDRH r2,[r0,#248]
        0xe182_2001, // ORR r2,r2,r1
        0xe1c0_2fb8, // STRH r2,[r0,#248] (BIOS flags)
        0xe12f_ff1e, // BX lr
    ]);
    let palette_address = ROM_START + (code.len() + 3 + usize::from(raster)) as u32 * 4;
    let mut literals = vec![(3, palette_address), (7, palette_address + 512), (11, irq)];
    if let Some(index) = raster_dma_load {
        literals.push((index, 0xa240_0001));
    }
    for (index, value) in literals {
        let offset = (code.len() - index - 2) * 4;
        assert!(offset < 4096);
        code[index] = 0xe59f_0000 | offset as u32;
        code.push(value);
    }
    let mut rom: Vec<u8> = code.into_iter().flat_map(u32::to_le_bytes).collect();
    if bitmap {
        rom.extend(crate::bitmap_demo::assets(mode));
        return rom;
    }
    let mut palette = [0_u16; 256];
    palette[..9].copy_from_slice(&[
        0x4000, 0x0260, 0x03a0, 0x7d20, 0x7e80, 0x001f, 0x421f, 0x03ff, 0x7fff,
    ]);
    rom.extend(palette.into_iter().flat_map(u16::to_le_bytes));
    let mut vram = vec![0_u8; 0x10000];
    for t in 0..4 {
        for y in 0..8 {
            for x in 0..8 {
                vram[t * 64 + y * 8 + x] = (t * 2 + 1 + (x / 2 + y / 2) % 2) as u8;
            }
        }
    }
    for y in 0..32 {
        for x in 0..32 {
            vram[0x8000 + y * 32 + x] = ((x / 4 + 2 * (y / 4)) % 4) as u8;
        }
    }
    rom.extend(vram);
    rom
}
