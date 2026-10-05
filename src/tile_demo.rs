//! Original Mode 0 scene. ARM code copies assets through BIOS services, polls
//! KEYINPUT, and updates scroll registers and a sprite once per VBlank.
//! The host does not write VRAM or object attribute memory (OAM).

use crate::{
    graphics_demo::{GraphicsDemo, GraphicsError},
    input::Buttons,
    machine::Machine,
    memory::{MemoryError, ROM_START},
    video::Framebuffer,
};

/// Completed updates, horizontal scroll, vertical scroll (three debug words).
pub const TILE_STATE: u32 = 0x0200_0000;

pub struct TileDemo {
    runner: GraphicsDemo,
}

impl TileDemo {
    pub fn new() -> Result<Self, MemoryError> {
        Ok(Self {
            runner: GraphicsDemo::with_program(program(Scene::Tiles))?,
        })
    }

    pub fn machine(&self) -> &Machine {
        self.runner.machine()
    }

    /// Run startup if needed, then present one completed VBlank-synchronized frame.
    pub fn frame(
        &mut self,
        buttons: Buttons,
        output: &mut Framebuffer,
    ) -> Result<usize, GraphicsError> {
        self.runner.frame_with_control(buttons, output, 0x1340)
    }
}

fn literal(code: &mut Vec<u32>, patches: &mut Vec<(usize, u32, u32)>, reg: u32, value: u32) {
    patches.push((code.len(), reg, value));
    code.push(0);
}

enum Scene {
    Tiles,
    Effects,
    Mosaic,
}

pub(crate) fn effects_program() -> Vec<u8> {
    program(Scene::Effects)
}

pub(crate) fn mosaic_program() -> Vec<u8> {
    program(Scene::Mosaic)
}

fn program(scene: Scene) -> Vec<u8> {
    let effects = matches!(scene, Scene::Effects);
    let mosaic = matches!(scene, Scene::Mosaic);
    let mut code = vec![
        0xe3a0_9301, // MOV r9,#0x04000000
        0xe3a0_0080, // MOV r0,#0x80 (forced blank)
        0xe1c9_00b0, // STRH r0,[r9]
    ];
    let mut patches = Vec::new();
    let palette_load = code.len();
    literal(&mut code, &mut patches, 0, 0); // LDR r0,=palette (patched after layout)
    code.extend([
        0xe3a0_1405, // MOV r1,#0x05000000
        0xe3a0_2c02, // MOV r2,#512 halfwords (BG and OBJ palettes)
        0xef0b_0000, // CpuSet
    ]);
    let vram_load = code.len();
    literal(&mut code, &mut patches, 0, 0); // LDR r0,=VRAM image
    code.extend([
        0xe3a0_1406, // MOV r1,#0x06000000
        0xe3a0_2a06, // MOV r2,#24576 words (96 KiB VRAM)
        0xef0c_0000, // CpuFastSet
    ]);
    let oam_load = code.len();
    literal(&mut code, &mut patches, 0, 0); // LDR r0,=OAM image
    code.extend([
        0xe3a0_1407, // MOV r1,#0x07000000
        0xe3a0_2c02, // MOV r2,#512 halfwords
        0xef0b_0000, // CpuSet; disables unused sprites
        0xe3a0_6407, // MOV r6,#0x07000000 (keep OAM base)
    ]);
    let irq_load = code.len();
    literal(&mut code, &mut patches, 0, 0);
    code.extend([
        0xe3a0_1403, // MOV r1,#0x03000000
        0xe281_1c7f, // ADD r1,r1,#0x7f00
        0xe581_00fc, // STR r0,[r1,#0xfc] (callback)
        0xe3a0_0008, // MOV r0,#8
        0xe1c9_00b4, // STRH r0,[r9,#4] (VBlank IRQ request)
        0xe289_1c02, // ADD r1,r9,#0x200
        0xe3a0_0001, // MOV r0,#1
        0xe1c1_00b0, // STRH r0,[r1] (IE)
    ]);
    literal(
        &mut code,
        &mut patches,
        0,
        if mosaic { 0x1844 } else { 0x1804 },
    ); // BG0: char1, map24
    code.push(0xe1c9_00b8); // STRH r0,[r9,#8]
    literal(
        &mut code,
        &mut patches,
        0,
        if mosaic { 0xd0c1 } else { 0xd081 },
    ); // BG1: 8bpp, 512x512
    code.extend([
        0xe1c9_00ba,                                     // STRH r0,[r9,#10]
        0xe289_ac01,                                     // ADD r10,r9,#0x100
        0xe28a_a030,                                     // ADD r10,r10,#0x30 (KEYINPUT)
        0xe3a0_b402,                                     // MOV r11,#0x02000000
        0xe3a0_4000,                                     // MOV r4,#0 (scroll x)
        0xe3a0_5000,                                     // MOV r5,#0 (scroll y)
        0xe3a0_7000,                                     // MOV r7,#0 (updates)
        if effects { 0xe3a0_0c33 } else { 0xe3a0_0c13 }, // WIN0 for effects; BG0+BG1+OBJ
        0xe380_0040,                                     // ORR r0,r0,#0x40 (1D OBJ mapping)
        0xe1c9_00b0,                                     // STRH r0,[r9]
    ]);
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
        0xe1a0_4b84,                                    // MOV r4,r4,LSL #23
        0xe1a0_4ba4,                                    // MOV r4,r4,LSR #23 (mask to 9 bits)
        0xe1a0_5b85,                                    // MOV r5,r5,LSL #23
        0xe1a0_5ba5,                                    // MOV r5,r5,LSR #23
        0xe1c9_41b4,                                    // STRH r4,[r9,#0x14] (BG1HOFS)
        0xe1c9_51b6,                                    // STRH r5,[r9,#0x16] (BG1VOFS)
        0xe1a0_00a4,                                    // MOV r0,r4,LSR #1
        0xe1a0_10a5,                                    // MOV r1,r5,LSR #1
        0xe1c9_01b0,                                    // STRH r0,[r9,#0x10] (BG0HOFS, half speed)
        0xe1c9_11b2,                                    // STRH r1,[r9,#0x12] (BG0VOFS)
        0xe3a0_0048,                                    // MOV r0,#72 (regular Y)
        0xe3a0_1901,                                    // MOV r1,#0x4000 (16x16 sprite)
        0xe381_1070,                                    // ORR r1,r1,#112 (regular X)
        0xe313_0001,                                    // TST r3,#A (Z)
        if mosaic { 0xe1a0_0000 } else { 0x0381_1a01 }, // Z flips except in mosaic demo
        0xe203_2c03,                                    // AND r2,r3,#0x300 (L/R are active-low)
        0xe352_0c03,                                    // CMP r2,#0x300
        0x13a0_0c03,                                    // MOVNE r0,#0x300 (affine + double-size)
        0x1380_0040, // ORRNE r0,r0,#64 (32-pixel canvas centered at y=80)
        0x13a0_1901, // MOVNE r1,#0x4000 (16x16 texture, matrix0; no flip)
        0x1381_1068, // ORRNE r1,r1,#104 (32-pixel canvas centered at x=120)
        0xe1c6_00b0, // STRH r0,[r6] (OBJ0 attribute0)
        0xe1c6_10b2, // STRH r1,[r6,#2] (OBJ0 attribute1)
        0xe3a0_0c01, // MOV r0,#256 (PA, identity)
        0xe3a0_1000, // MOV r1,#0 (PB)
        0xe3a0_cc01, // MOV r12,#256 (PD)
        0xe313_0c02, // TST r3,#L (Q: 45-degree clockwise rotation)
        0x03a0_00b5, // MOVEQ r0,#181
        0x03a0_10b5, // MOVEQ r1,#181
        0x03a0_c0b5, // MOVEQ r12,#181
        0xe313_0c01, // TST r3,#R (W: approximately 2x scale)
        0x01a0_00a0, // MOVEQ r0,r0,LSR #1
        0x01a0_10a1, // MOVEQ r1,r1,LSR #1
        0x01a0_c0ac, // MOVEQ r12,r12,LSR #1
        0xe261_2000, // RSB r2,r1,#0 (PC=-PB, after scale quantization)
        0xe1c6_00b6, // STRH r0,[r6,#6] (matrix0 PA)
        0xe1c6_10be, // STRH r1,[r6,#14] (matrix0 PB)
        0xe1c6_21b6, // STRH r2,[r6,#22] (matrix0 PC)
        0xe1c6_c1be, // STRH r12,[r6,#30] (matrix0 PD)
        0xe3a0_1000, // MOV r1,#0 (priority0, tile0)
        0xe313_0002, // TST r3,#B (X)
        if mosaic { 0xe1a0_0000 } else { 0x03a0_1b01 }, // X lowers priority except in mosaic demo
        0xe1c6_10b4, // STRH r1,[r6,#4] (OBJ0 attribute2)
        0xe287_7001, // ADD r7,r7,#1
        0xe58b_7000, // STR r7,[r11]
        0xe58b_4004, // STR r4,[r11,#4]
        0xe58b_5008, // STR r5,[r11,#8]
    ]);
    if mosaic {
        code.extend([
            0xe1a0_01a7, // MOV r0,r7,LSR #3 (advance size every eight updates)
            0xe200_000f, // AND r0,r0,#15 (encoded size minus one)
            0xe180_0200, // ORR r0,r0,r0,LSL #4
            0xe180_0400, // ORR r0,r0,r0,LSL #8 (same size for all fields)
            0xe313_0001, // TST r3,#A (Z: BG1x1)
            0x03c0_00ff, // BICEQ r0,r0,#0xff
            0xe313_0002, // TST r3,#B (X: OBJ1x1)
            0x03c0_0cff, // BICEQ r0,r0,#0xff00
            0xe1c9_04bc, // STRH r0,[r9,#0x4c] (MOSAIC)
            0xe1d6_10b0, // LDRH r1,[r6] (OBJ attribute0)
            0xe381_1a01, // ORR r1,r1,#0x1000 (mosaic, regular or affine)
            0xe1c6_10b0, // STRH r1,[r6]
        ]);
    }
    if effects {
        code.extend([
            0xe284_0040, // ADD r0,r4,#64 (window left, follows horizontal scroll)
            0xe200_00ff, // AND r0,r0,#255
            0xe280_1070, // ADD r1,r0,#112 (window width)
            0xe201_10ff, // AND r1,r1,#255
            0xe181_1400, // ORR r1,r1,r0,LSL #8
            0xe1c9_14b0, // STRH r1,[r9,#0x40] (WIN0H)
            0xe285_0028, // ADD r0,r5,#40 (window top)
            0xe200_00ff, // AND r0,r0,#255
            0xe280_1050, // ADD r1,r0,#80 (window height)
            0xe201_10ff, // AND r1,r1,#255
            0xe181_1400, // ORR r1,r1,r0,LSL #8
            0xe1c9_14b4, // STRH r1,[r9,#0x44] (WIN0V)
            0xe3a0_001f, // MOV r0,#0x1f (all layers, no effects inside)
            0xe1c9_04b8, // STRH r0,[r9,#0x48] (WININ)
            0xe3a0_003f, // MOV r0,#0x3f (all layers and effects outside)
            0xe1c9_04ba, // STRH r0,[r9,#0x4a] (WINOUT)
            0xe3a0_00bf, // MOV r0,#0xbf (brighten all layers)
            0xe313_0002, // TST r3,#B (X: darken)
            0x03a0_00ff, // MOVEQ r0,#0xff
            0xe313_0001, // TST r3,#A (Z: alpha BG0/OBJ over BG1)
            0x03a0_0c02, // MOVEQ r0,#0x200
            0x0380_0051, // ORREQ r0,r0,#0x51
            0xe1c9_05b0, // STRH r0,[r9,#0x50] (BLDCNT)
            0xe3a0_0c08, // MOV r0,#0x800
            0xe380_0008, // ORR r0,r0,#8
            0xe1c9_05b2, // STRH r0,[r9,#0x52] (BLDALPHA: half of each target)
            0xe3a0_0008, // MOV r0,#8
            0xe1c9_05b4, // STRH r0,[r9,#0x54] (BLDY: half brightness change)
        ]);
    }
    let offset = frame as i32 - code.len() as i32 - 2;
    code.push(0xea00_0000 | (offset as u32 & 0x00ff_ffff));
    let irq_address = ROM_START + code.len() as u32 * 4;
    code.extend([
        0xe280_0c02, // ADD r0,r0,#0x200 (BIOS supplies I/O base)
        0xe1d0_10b2, // LDRH r1,[r0,#2]
        0xe1c0_10b2, // STRH r1,[r0,#2] (ack IF)
        0xe3a0_0403, // MOV r0,#0x03000000
        0xe280_0c7f, // ADD r0,r0,#0x7f00
        0xe1d0_2fb8, // LDRH r2,[r0,#0xf8]
        0xe182_2001, // ORR r2,r2,r1
        0xe1c0_2fb8, // STRH r2,[r0,#0xf8] (BIOS flags)
        0xe12f_ff1e, // BX lr
    ]);
    let palette_address = ROM_START + (code.len() + patches.len()) as u32 * 4;
    for (index, reg, mut value) in patches {
        if index == palette_load {
            value = palette_address;
        }
        if index == vram_load {
            value = palette_address + 1024;
        }
        if index == oam_load {
            value = palette_address + 1024 + 0x18000;
        }
        if index == irq_load {
            value = irq_address;
        }
        let offset = (code.len() - index - 2) * 4;
        assert!(offset < 4096);
        code[index] = 0xe59f_0000 | (reg << 12) | offset as u32;
        code.push(value);
    }
    let mut rom: Vec<u8> = code.into_iter().flat_map(u32::to_le_bytes).collect();
    let mut palette = [0_u16; 512];
    palette[0] = 0x4000;
    palette[1] = 0x0260; // Grass
    palette[2] = 0x03a0; // Light grass
    palette[3] = 0x7d20; // Water
    palette[4] = 0x7e80; // Light water
    palette[17] = 0x03ff; // Foreground gold (bank1)
    palette[257] = 0x001f; // Sprite red
    palette[258] = 0x7fff; // Sprite white
    palette[259] = 0; // Sprite opaque black eye
    palette[260] = 0x03ff; // Sprite gold arm
    rom.extend(palette.into_iter().flat_map(u16::to_le_bytes));
    let mut vram = vec![0_u8; 0x18000];
    for tile in 0..2 {
        for y in 0..8 {
            for x in 0..8 {
                vram[tile * 64 + y * 8 + x] = (tile * 2 + 1 + (x / 2 + y / 2) % 2) as u8;
            }
        }
    }
    // Sparse foreground crosses. Index zero leaves the terrain visible.
    for y in 0..8 {
        for x in 0..8 {
            if x == 3 || y == 3 {
                vram[0x4020 + y * 4 + x / 2] |= 1 << ((x % 2) * 4);
            }
        }
    }
    for y in 0..64 {
        for x in 0..64 {
            let block = x / 32 + y / 32 * 2;
            let address = 0x8000 + block * 0x800 + (y % 32 * 32 + x % 32) * 2;
            let entry = ((x / 4 + y / 4) % 2) as u16;
            vram[address..address + 2].copy_from_slice(&entry.to_le_bytes());
        }
    }
    for y in 0..32 {
        for x in 0..32 {
            let entry = if x % 4 == 1 && y % 4 == 1 {
                0x1001_u16
            } else {
                0
            };
            let address = 0xc000 + (y * 32 + x) * 2;
            vram[address..address + 2].copy_from_slice(&entry.to_le_bytes());
        }
    }
    // Original 16x16 character, stored as four consecutive 4bpp tiles.
    for y in 1..15 {
        for x in 2..14 {
            let color = if x == 10 && y == 7 {
                3
            } else if x == 12 && (10..13).contains(&y) {
                4
            } else if (5..10).contains(&y) {
                2
            } else {
                1
            };
            let tile = y / 8 * 2 + x / 8;
            let address = 0x10000 + tile * 32 + (y % 8) * 4 + (x % 8) / 2;
            vram[address] |= color << ((x & 1) * 4);
        }
    }
    rom.extend(vram);
    let mut oam = [0_u16; 512];
    for object in 0..128 {
        oam[object * 4] = 0x200;
    }
    oam[0] = 72; // Y
    oam[1] = 0x4070; // 16x16, X=112
    oam[2] = 0; // Tile0, priority0, palette0
    rom.extend(oam.into_iter().flat_map(u16::to_le_bytes));
    rom
}
