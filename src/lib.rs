//! An educational GBA starting point, not a game-compatible emulator.

pub mod affine_demo;
pub mod affine_raster_demo;
pub mod bios;
pub mod bitmap_demo;
pub mod cpu;
pub mod display;
pub mod dma;
pub mod effects_demo;
pub mod graphics_demo;
pub mod input;
pub mod io;
pub mod machine;
pub mod memory;
pub mod mosaic_demo;
pub mod raster_demo;
pub mod tile_demo;
pub mod timer_demo;
pub mod timing;
pub mod video;

/// Steps needed to reach and execute the demo's final self-branch.
pub const DEMO_STEPS: usize = 40;

/// Steps needed to execute both software interrupts and the final self-branch.
pub const EXCEPTION_DEMO_STEPS: usize = 11;

/// Original test image, not a Nintendo BIOS or a BIOS-service replacement.
/// The SWI vector branches to a handler that increments r10 and returns.
pub fn demo_bios() -> Vec<u8> {
    let mut bytes = vec![0; memory::BIOS_SIZE];
    bytes[8..12].copy_from_slice(&0xea00_000c_u32.to_le_bytes()); // B 0x40
    bytes[0x40..0x44].copy_from_slice(&0xe28a_a001_u32.to_le_bytes()); // ADD r10, r10, #1
    bytes[0x44..0x48].copy_from_slice(&0xe1b0_f00e_u32.to_le_bytes()); // MOVS pc, lr
    bytes
}

/// Original ARM and Thumb software-interrupt test code.
pub fn exception_demo_program() -> Vec<u8> {
    let mut bytes: Vec<u8> = [
        0xef00_0001_u32, // ARM SWI #1
        0xe28f_0001,     // ADD r0, pc, #1 (Thumb offset 12, bit zero set)
        0xe12f_ff10,     // BX r0
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    bytes.extend(0xdf02_u16.to_le_bytes()); // Thumb SWI #2
    bytes.extend(0xe7fe_u16.to_le_bytes()); // B .
    bytes
}

/// Original ARM/Thumb, RAM, stack, and swap test code, not a Nintendo ROM.
pub fn demo_program() -> Vec<u8> {
    let mut bytes: Vec<u8> = [
        0xe3a0_0003_u32, // MOV r0, #3
        0xe3a0_1000,     // MOV r1, #0
        0xe281_1001,     // loop: ADD r1, r1, #1
        0xe250_0001,     // SUBS r0, r0, #1
        0x1aff_fffc,     // BNE loop
        0xe351_0003,     // CMP r1, #3
        0x03a0_202a,     // MOVEQ r2, #42
        0xe3a0_3402,     // MOV r3, #0x02000000 (work RAM)
        0xe583_2000,     // STR r2, [r3]
        0xe593_4000,     // LDR r4, [r3]
        0xe3a0_d403,     // MOV sp, #0x03000000 (internal work RAM)
        0xe28d_dc01,     // ADD sp, sp, #0x100
        0xeb00_0000,     // BL subroutine at offset 56
        0xea00_0005,     // B Thumb setup at offset 80
        0xe92d_4010,     // subroutine: STMDB sp!, {r4, lr}
        0xe1a0_5084,     // MOV r5, r4, LSL #1
        0xe006_0594,     // MUL r6, r4, r5
        0xe103_7096,     // SWP r7, r6, [r3] (old RAM value into r7)
        0xe3a0_4000,     // MOV r4, #0 (demonstrate restoration from the stack)
        0xe8bd_8010,     // LDMIA sp!, {r4, pc} (restore r4 and return)
        0xe28f_800d,     // ADD r8, pc, #13 (Thumb entry at offset 100, bit 0 set)
        0xe28f_9004,     // ADD r9, pc, #4 (ARM return at offset 96)
        0xe12f_ff18,     // BX r8
        0xe1a0_0000,     // NOP, skipped
        0xeaff_fffe,     // ARM return: B .
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    bytes.extend(
        [
            0x2006_u16, // Thumb entry: MOV r0, #6
            0x2107,     // MOV r1, #7
            0xf000,     // BL prefix
            0xf803,     // BL suffix: target offset 114
            0x6058,     // STR r0, [r3, #4]
            0x282a,     // CMP r0, #42
            0x4748,     // BX r9 (return to ARM)
            0xb500,     // Thumb subroutine: PUSH {lr}
            0x4348,     // MUL r0, r1
            0xbd00,     // POP {pc} (stays in Thumb)
        ]
        .into_iter()
        .flat_map(u16::to_le_bytes),
    );
    bytes
}
