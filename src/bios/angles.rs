//! Original ARM implementations of the BIOS fixed-point angle services.
//! Inputs must be sign-extended signed 16-bit values. Only r0 is returned;
//! undocumented scratch-register results and original BIOS timing are not modeled.

use super::ArmImage;

pub(super) fn emit(a: &mut ArmImage) {
    a.label("arctan");
    validate_input(a, 0);
    a.call("atan_polynomial");
    a.emit(0xe58d_0004); // STR r0,[sp,#4] (saved caller r0)
    a.branch(14, "return");

    a.label("arctan2");
    validate_input(a, 0);
    validate_input(a, 1);
    a.emit(0xe92d_0070); // STMDB sp!,{r4-r6} (40 bytes total SVC stack use)
    a.emit(0xe351_0000); // CMP r1,#0 (Y)
    a.branch(0, "atan2_horizontal_axis");
    a.emit(0xe350_0000); // CMP r0,#0 (X)
    a.branch(0, "atan2_vertical_axis");
    a.emit(0xe1a0_4000); // MOV r4,r0 (original X)
    a.emit(0xe1a0_5001); // MOV r5,r1 (original Y)
    a.emit(0xe020_6001); // EOR r6,r0,r1 (ratio sign in bit31)
    a.emit(0xe350_0000); // CMP r0,#0
    a.emit(0x4260_0000); // RSBMI r0,r0,#0 (|X|, including -32768)
    a.emit(0xe351_0000); // CMP r1,#0
    a.emit(0x4261_1000); // RSBMI r1,r1,#0 (|Y|)
    a.emit(0xe150_0001); // CMP r0,r1
    a.branch(3, "atan2_vertical_ratio"); // BLO |X| < |Y|

    // Horizontal sectors: ArcTan(Y/X) plus 0 or half a turn.
    a.emit(0xe1a0_2000); // MOV r2,r0
    a.emit(0xe1a0_0701); // MOV r0,r1,LSL #14 (unsigned numerator)
    a.emit(0xe1a0_1002); // MOV r1,r2 (unsigned denominator)
    a.emit(0xe354_0000); // CMP r4,#0 (original X)
    a.emit(0xa3a0_4000); // MOVGE r4,#0
    a.emit(0xb3a0_4902); // MOVLT r4,#0x8000
    a.emit(0xe3a0_5000); // MOV r5,#0 (add angle to base)
    a.branch(14, "atan2_divide");

    a.label("atan2_vertical_ratio");
    // Vertical sectors: quarter/three-quarter turn minus ArcTan(X/Y).
    a.emit(0xe1a0_0700); // MOV r0,r0,LSL #14
    a.emit(0xe355_0000); // CMP r5,#0 (original Y)
    a.emit(0xa3a0_4901); // MOVGE r4,#0x4000
    a.emit(0xb3a0_4903); // MOVLT r4,#0xc000
    a.emit(0xe3a0_5001); // MOV r5,#1 (subtract angle from base)

    a.label("atan2_divide");
    // Binary long division of (smaller magnitude << 14) by larger magnitude.
    // Axis cases excluded zero denominators; the quotient is at most 0x4000.
    a.emit(0xe3a0_2000); // MOV r2,#0 (quotient)
    a.emit(0xe3a0_3000); // MOV r3,#0 (remainder)
    a.emit(0xe3a0_c020); // MOV r12,#32
    a.label("atan2_divide_bit");
    a.emit(0xe1b0_0080); // MOVS r0,r0,LSL #1
    a.emit(0xe0a3_3003); // ADC r3,r3,r3
    a.emit(0xe153_0001); // CMP r3,r1
    a.emit(0x2043_3001); // SUBCS r3,r3,r1
    a.emit(0xe0a2_2002); // ADC r2,r2,r2
    a.emit(0xe25c_c001); // SUBS r12,r12,#1
    a.branch(1, "atan2_divide_bit");
    a.emit(0xe1a0_0002); // MOV r0,r2
    a.emit(0xe316_0102); // TST r6,#0x80000000
    a.emit(0x1260_0000); // RSBNE r0,r0,#0 (signed ratio, truncated toward zero)
    a.call("atan_polynomial");
    a.emit(0xe355_0000); // CMP r5,#0
    a.emit(0x0084_0000); // ADDEQ r0,r4,r0
    a.emit(0x1060_0004); // RSBNE r0,r0,r4
    a.branch(14, "atan2_done");

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
    a.emit(0xe8bd_0070); // LDMIA sp!,{r4-r6}
    a.branch(14, "return");

    // Shared leaf kernel: r0 is signed Q2.14; r1-r3 are scratch.
    // Every multiply keeps its low 32 bits before the arithmetic shift.
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
    a.emit(0xe1a0_0843); // MOV r0,r3,ASR #16 (sign-extended 16-bit angle)
    a.emit(0xe12f_ff1e); // BX lr
}

fn validate_input(a: &mut ArmImage, register: u32) {
    a.emit(0xe1a0_2800 | register); // MOV r2,rN,LSL #16
    a.emit(0xe1a0_2842); // MOV r2,r2,ASR #16
    a.emit(0xe150_0002 | register << 16); // CMP rN,r2
    a.branch(1, "invalid_argument");
}
