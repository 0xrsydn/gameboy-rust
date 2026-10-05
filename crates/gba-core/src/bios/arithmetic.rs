//! Original integer algorithms emitted as ARM instructions, not host calculations.

use super::ArmImage;

pub(super) fn emit(a: &mut ArmImage) {
    a.label("div_arm");
    a.emit(0xe1a0_2000); // MOV r2,r0
    a.emit(0xe1a0_0001); // MOV r0,r1
    a.emit(0xe1a0_1002); // MOV r1,r2 (exchange input operands)
    a.label("div");
    a.emit(0xe351_0000); // CMP r1,#0
    a.branch(0, "invalid_argument"); // Diagnostic instead of firmware's divide-by-zero hang.
    a.emit(0xe92d_0070); // STMDB sp!,{r4-r6} (40 bytes total SVC stack use)
    a.emit(0xe1a0_4fc0); // MOV r4,r0,ASR #31 (numerator sign)
    a.emit(0xe020_5001); // EOR r5,r0,r1
    a.emit(0xe1a0_5fc5); // MOV r5,r5,ASR #31 (quotient sign)
    a.emit(0xe350_0000); // CMP r0,#0
    a.emit(0x4260_0000); // RSBMI r0,r0,#0 (unsigned magnitude, including INT_MIN)
    a.emit(0xe351_0000); // CMP r1,#0
    a.emit(0x4261_1000); // RSBMI r1,r1,#0
    a.emit(0xe3a0_2000); // MOV r2,#0 (quotient magnitude)
    a.emit(0xe3a0_3000); // MOV r3,#0 (remainder magnitude)
    a.emit(0xe3a0_6020); // MOV r6,#32
    a.label("div_bit");
    a.emit(0xe1b0_0080); // MOVS r0,r0,LSL #1 (next numerator bit into C)
    a.emit(0xe0a3_3003); // ADC r3,r3,r3 (remainder * 2 + bit)
    a.emit(0xe153_0001); // CMP r3,r1 (unsigned)
    a.emit(0x2043_3001); // SUBCS r3,r3,r1 (preserve comparison carry)
    a.emit(0xe0a2_2002); // ADC r2,r2,r2 (quotient * 2 + comparison carry)
    a.emit(0xe256_6001); // SUBS r6,r6,#1
    a.branch(1, "div_bit");
    // Patch only the defined outputs in the dispatcher's saved register frame.
    a.emit(0xe58d_201c); // STR r2,[sp,#28] (saved r3 = unsigned quotient magnitude)
    a.emit(0xe355_0000); // CMP r5,#0
    a.emit(0x1262_2000); // RSBNE r2,r2,#0 (signed quotient)
    a.emit(0xe354_0000); // CMP r4,#0
    a.emit(0x1263_3000); // RSBNE r3,r3,#0 (remainder follows numerator sign)
    a.emit(0xe58d_2010); // STR r2,[sp,#16] (saved r0)
    a.emit(0xe58d_3014); // STR r3,[sp,#20] (saved r1)
    a.emit(0xe8bd_0070); // LDMIA sp!,{r4-r6}
    a.branch(14, "return");

    // Restoring base-four square root: exactly 16 iterations for any u32 input.
    a.label("sqrt");
    a.emit(0xe3a0_1000); // MOV r1,#0 (root)
    a.emit(0xe3a0_2101); // MOV r2,#0x40000000 (highest power of four)
    a.label("sqrt_bit");
    a.emit(0xe081_3002); // ADD r3,r1,r2 (trial)
    a.emit(0xe150_0003); // CMP r0,r3
    a.emit(0x2040_0003); // SUBCS r0,r0,r3
    a.emit(0xe1a0_10a1); // MOV r1,r1,LSR #1 (comparison carry unchanged)
    a.emit(0x2081_1002); // ADDCS r1,r1,r2
    a.emit(0xe1b0_2122); // MOVS r2,r2,LSR #2
    a.branch(1, "sqrt_bit");
    a.emit(0xe58d_1004); // STR r1,[sp,#4] (saved r0 = floor(sqrt(input)))
    a.branch(14, "return");
}
