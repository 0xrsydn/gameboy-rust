//! Original ARM raster program. Repeated HBlank DMA changes the palette backdrop
//! after each captured row. A whole-frame snapshot cannot reproduce these bands.

use crate::graphics_demo::{GraphicsDemo, GraphicsError};
use gba_core::{
    input::Buttons,
    machine::Machine,
    memory::{MemoryError, ROM_START},
    video::Framebuffer,
};

/// Completed updates and five-bit band phase, stored as two debug words.
pub const RASTER_STATE: u32 = 0x0200_0000;
pub const RASTER_TABLE: u32 = 0x0200_1000;

pub struct RasterDemo {
    runner: GraphicsDemo,
}

impl RasterDemo {
    pub fn new() -> Result<Self, MemoryError> {
        Ok(Self {
            runner: GraphicsDemo::with_program(program())?,
        })
    }

    pub fn machine(&self) -> &Machine {
        self.runner.machine()
    }

    /// Left/right move the color bands. Enter resets their phase.
    pub fn frame(
        &mut self,
        buttons: Buttons,
        output: &mut Framebuffer,
    ) -> Result<usize, GraphicsError> {
        self.runner.frame_with_control(buttons, output, 0x400)
    }
}

fn literal(code: &mut Vec<u32>, pool: &mut Vec<(usize, u32, u32)>, register: u32, value: u32) {
    pool.push((code.len(), register, value));
    code.push(0);
}

fn program() -> Vec<u8> {
    let mut code = vec![
        0xe3a0_9301, // MOV r9,#0x04000000
        0xe3a0_0080, // MOV r0,#128 (forced blank)
        0xe1c9_00b0, // STRH r0,[r9]
    ];
    let mut pool = Vec::new();
    let table_load = code.len();
    literal(&mut code, &mut pool, 0, 0);
    code.extend([
        0xe3a0_1402, // MOV r1,#0x02000000
        0xe281_1a01, // ADD r1,r1,#0x1000
        0xe3a0_2c01, // MOV r2,#256 halfwords
        0xef0b_0000, // CpuSet: copy original color table to EWRAM
    ]);
    let irq_load = code.len();
    literal(&mut code, &mut pool, 0, 0);
    code.extend([
        0xe3a0_1403, // MOV r1,#0x03000000
        0xe281_1c7f, // ADD r1,r1,#0x7f00
        0xe581_00fc, // STR r0,[r1,#0xfc] (IRQ callback)
        0xe3a0_0008, // MOV r0,#8
        0xe1c9_00b4, // STRH r0,[r9,#4] (VBlank IRQ)
        0xe289_1c02, // ADD r1,r9,#0x200
        0xe3a0_0001, // MOV r0,#1
        0xe1c1_00b0, // STRH r0,[r1] (IE)
        0xe3a0_8402, // MOV r8,#0x02000000
        0xe288_8a01, // ADD r8,r8,#0x1000 (table base)
        0xe3a0_6405, // MOV r6,#0x05000000 (backdrop palette)
        0xe289_50b0, // ADD r5,r9,#0xb0 (DMA0 registers)
        0xe289_ac01, // ADD r10,r9,#0x100
        0xe28a_a030, // ADD r10,r10,#0x30 (KEYINPUT)
        0xe3a0_b402, // MOV r11,#0x02000000 (debug state)
        0xe3a0_4000, // MOV r4,#0 (phase)
        0xe3a0_7000, // MOV r7,#0 (updates)
        0xe3a0_0b01, // MOV r0,#0x400 (Mode0, transparent BG2)
        0xe1c9_00b0, // STRH r0,[r9] (startup complete)
    ]);
    let frame = code.len();
    code.extend([
        0xef05_0000, // VBlankIntrWait
        0xe1da_30b0, // LDRH r3,[r10]
        0xe313_0010, // TST r3,#Right
        0x0284_4001, // ADDEQ r4,r4,#1
        0xe313_0020, // TST r3,#Left
        0x0244_4001, // SUBEQ r4,r4,#1
        0xe204_401f, // AND r4,r4,#31
        0xe313_0008, // TST r3,#Start
        0x03a0_4000, // MOVEQ r4,#0
        0xe3a0_0000, // MOV r0,#0
        0xe1c5_00ba, // STRH r0,[r5,#10] (disable before rearming DMA)
        0xe088_0084, // ADD r0,r8,r4,LSL #1
        0xe1d0_10b0, // LDRH r1,[r0] (color for row0)
        0xe1c6_10b0, // STRH r1,[r6]
        0xe280_0002, // ADD r0,r0,#2 (row1 color starts HBlank stream)
        0xe585_0000, // STR r0,[r5] (source)
        0xe585_6004, // STR r6,[r5,#4] (fixed destination)
    ]);
    literal(&mut code, &mut pool, 0, 0xa240_0001); // HBlank, repeat, fixed destination, one halfword.
    code.extend([
        0xe585_0008, // STR r0,[r5,#8] (arm DMA0)
        0xe287_7001, // ADD r7,r7,#1
        0xe58b_7000, // STR r7,[r11]
        0xe58b_4004, // STR r4,[r11,#4]
    ]);
    let displacement = frame as i32 - code.len() as i32 - 2;
    code.push(0xea00_0000 | (displacement as u32 & 0x00ff_ffff));
    let irq = ROM_START + code.len() as u32 * 4;
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
    let table = ROM_START + (code.len() + pool.len()) as u32 * 4;
    for (index, register, value) in pool {
        let value = if index == table_load {
            table
        } else if index == irq_load {
            irq
        } else {
            value
        };
        let offset = (code.len() - index - 2) * 4;
        assert!(offset < 4096);
        code[index] = 0xe59f_0000 | register << 12 | offset as u32;
        code.push(value);
    }
    let mut rom: Vec<u8> = code.into_iter().flat_map(u32::to_le_bytes).collect();
    for i in 0..256u16 {
        let n = i & 31;
        let color = n | ((31 - n) << 5) | ((n / 2) << 10);
        rom.extend(color.to_le_bytes());
    }
    rom
}
