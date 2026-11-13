//! Original ARM implementations of the BIOS fixed-point angle services.
//! Results, including the r1/r3 side effects, follow the hardware expectations of the
//! mGBA test suite; the algorithm follows mGBA's high-level model of the firmware.
//! Original BIOS timing is not modeled.

use super::ArmImage;

/// ArcTan2 leaves this value in r3 on hardware; it is not an address in this image.
pub(super) const ARCTAN2_R3: u32 = 0x170;

pub(super) fn emit(a: &mut ArmImage) {
    // Any 32-bit input is accepted; products keep their low 32 bits as on hardware.
    a.label("arctan");
    a.call("atan_polynomial");
    a.emit(0xe58d_0004); // STR r0,[sp,#4] (saved caller r0)
    a.emit(0xe58d_1008); // STR r1,[sp,#8] (r1 = negated squared ratio)
    a.emit(0xe58d_2010); // STR r2,[sp,#16] (r3 = polynomial sum)
    a.branch(14, "return");

    a.label("arctan2");
    a.emit(0xe92d_0070); // STMDB sp!,{r4-r6} (40 bytes total SVC stack use)
    a.emit(0xe351_0000); // CMP r1,#0 (Y)
    a.branch(0, "atan2_horizontal_axis");
    a.emit(0xe350_0000); // CMP r0,#0 (X)
    a.branch(0, "atan2_vertical_axis");
    a.emit(0xe260_2000); // RSB r2,r0,#0 (-X)
    a.emit(0xe261_3000); // RSB r3,r1,#0 (-Y)
                         // Signed comparisons choose the sector exactly as the firmware does, ties included.
    a.emit(0xe351_0000); // CMP r1,#0
    a.branch(11, "atan2_lower"); // BLT
    a.emit(0xe350_0000); // CMP r0,#0
    a.branch(11, "atan2_upper_left"); // BLT
    a.emit(0xe150_0001); // CMP r0,r1 (X >= Y)
    a.emit(0xa3a0_4000); // MOVGE r4,#0
    a.branch(10, "atan2_horizontal");
    a.emit(0xe3a0_4901); // MOV r4,#0x4000
    a.branch(14, "atan2_vertical");
    a.label("atan2_upper_left");
    a.emit(0xe152_0001); // CMP r2,r1 (-X >= Y)
    a.emit(0xa3a0_4902); // MOVGE r4,#0x8000
    a.branch(10, "atan2_horizontal");
    a.emit(0xe3a0_4901); // MOV r4,#0x4000
    a.branch(14, "atan2_vertical");
    a.label("atan2_lower");
    a.emit(0xe350_0000); // CMP r0,#0
    a.branch(12, "atan2_lower_right"); // BGT
    a.emit(0xe152_0003); // CMP r2,r3 (-X > -Y)
    a.emit(0xc3a0_4902); // MOVGT r4,#0x8000
    a.branch(12, "atan2_horizontal");
    a.emit(0xe3a0_4903); // MOV r4,#0xc000
    a.branch(14, "atan2_vertical");
    a.label("atan2_lower_right");
    a.emit(0xe150_0003); // CMP r0,r3 (X >= -Y)
    a.emit(0xa3a0_4801); // MOVGE r4,#0x10000
    a.branch(10, "atan2_horizontal");
    a.emit(0xe3a0_4903); // MOV r4,#0xc000
    a.branch(14, "atan2_vertical");

    a.label("atan2_horizontal");
    // Base plus ArcTan((Y << 14) / X).
    a.emit(0xe1a0_2701); // MOV r2,r1,LSL #14
    a.emit(0xe1a0_3000); // MOV r3,r0
    a.emit(0xe3a0_5000); // MOV r5,#0 (add angle to base)
    a.branch(14, "atan2_divide");
    a.label("atan2_vertical");
    // Base minus ArcTan((X << 14) / Y).
    a.emit(0xe1a0_2700); // MOV r2,r0,LSL #14
    a.emit(0xe1a0_3001); // MOV r3,r1
    a.emit(0xe3a0_5001); // MOV r5,#1 (subtract angle from base)

    a.label("atan2_divide");
    // Signed 32-bit division truncated toward zero; both axis cases excluded zero divisors.
    a.emit(0xe022_6003); // EOR r6,r2,r3 (quotient sign in bit 31)
    a.emit(0xe352_0000); // CMP r2,#0
    a.emit(0x4262_2000); // RSBMI r2,r2,#0 (unsigned magnitude, including INT_MIN)
    a.emit(0xe353_0000); // CMP r3,#0
    a.emit(0x4263_3000); // RSBMI r3,r3,#0
    a.emit(0xe3a0_0000); // MOV r0,#0 (quotient)
    a.emit(0xe3a0_1000); // MOV r1,#0 (remainder)
    a.emit(0xe3a0_c020); // MOV r12,#32
    a.label("atan2_divide_bit");
    a.emit(0xe1b0_2082); // MOVS r2,r2,LSL #1
    a.emit(0xe0a1_1001); // ADC r1,r1,r1
    a.emit(0xe151_0003); // CMP r1,r3
    a.emit(0x2041_1003); // SUBCS r1,r1,r3
    a.emit(0xe0a0_0000); // ADC r0,r0,r0
    a.emit(0xe25c_c001); // SUBS r12,r12,#1
    a.branch(1, "atan2_divide_bit");
    a.emit(0xe356_0000); // CMP r6,#0
    a.emit(0x4260_0000); // RSBMI r0,r0,#0
    a.call("atan_polynomial");
    a.emit(0xe355_0000); // CMP r5,#0
    a.emit(0x0084_0000); // ADDEQ r0,r4,r0
    a.emit(0x1060_0004); // RSBNE r0,r0,r4
    a.emit(0xe58d_1014); // STR r1,[sp,#20] (saved r1 = inner negated squared ratio)
    a.branch(14, "atan2_done");

    // Axis directions leave r1 unchanged.
    a.label("atan2_horizontal_axis");
    a.emit(0xe350_0000); // CMP r0,#0
    a.emit(0xa3a0_0000); // MOVGE r0,#0 (includes X=Y=0)
    a.emit(0xb3a0_0902); // MOVLT r0,#0x8000
    a.branch(14, "atan2_done");
    a.label("atan2_vertical_axis");
    a.emit(0xe351_0000); // CMP r1,#0
    a.emit(0xa3a0_0901); // MOVGE r0,#0x4000
    a.emit(0xb3a0_0903); // MOVLT r0,#0xc000
    a.label("atan2_done");
    a.emit(0xe1a0_0800); // MOV r0,r0,LSL #16
    a.emit(0xe1a0_0820); // MOV r0,r0,LSR #16 (unsigned turn angle)
    a.emit(0xe58d_0010); // STR r0,[sp,#16] (saved caller r0 below local frame)
    a.literal(3, ARCTAN2_R3);
    a.emit(0xe58d_301c); // STR r3,[sp,#28] (saved r3)
    a.emit(0xe8bd_0070); // LDMIA sp!,{r4-r6}
    a.branch(14, "return");

    // Shared leaf kernel: r0 is signed Q2.14. Returns r1 = -(r0*r0 >> 14) and r2 = the
    // polynomial sum; r3 is scratch. Every multiply keeps its low 32 bits before the shift.
    a.label("atan_polynomial");
    a.emit(0xe001_0090); // MUL r1,r0,r0
    a.emit(0xe1a0_1741); // MOV r1,r1,ASR #14
    a.emit(0xe261_1000); // RSB r1,r1,#0 (negative squared ratio)
    a.emit(0xe3a0_20a9); // MOV r2,#0xa9
    for coefficient in [0x390, 0x91c, 0xfb6, 0x16aa, 0x2081, 0x3651, 0xa2f9] {
        a.emit(0xe003_0192); // MUL r3,r2,r1
        a.emit(0xe1a0_2743); // MOV r2,r3,ASR #14
        a.literal(3, coefficient);
        a.emit(0xe082_2003); // ADD r2,r2,r3
    }
    a.emit(0xe003_0290); // MUL r3,r0,r2
    a.emit(0xe1a0_0843); // MOV r0,r3,ASR #16 (signed angle)
    a.emit(0xe12f_ff1e); // BX lr
}
