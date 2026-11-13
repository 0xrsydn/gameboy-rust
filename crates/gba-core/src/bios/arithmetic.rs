//! Original integer algorithms emitted as ARM instructions, not host calculations.

use super::{pad, ArmImage};

pub(super) fn emit(a: &mut ArmImage) {
    a.label("div_arm");
    a.emit(0xe1a0_2000); // MOV r2,r0
    a.emit(0xe1a0_0001); // MOV r0,r1
    a.emit(0xe1a0_1002); // MOV r1,r2 (exchange input operands)
    a.label("div");
    a.emit(0xe351_0000); // CMP r1,#0
    a.branch(0, "div_zero");
    pad(a, DIV_PAD);
    // Take magnitudes (INT_MIN included). r12 bit 31 becomes the quotient sign and
    // bit 0 the numerator (remainder) sign.
    a.emit(0xe211_c102); // ANDS r12,r1,#0x80000000
    a.emit(0x4261_1000); // RSBMI r1,r1,#0
    a.emit(0xe03c_c040); // EORS r12,r12,r0,ASR #32 (C = numerator sign)
    a.emit(0x2260_0000); // RSBCS r0,r0,#0
                         // Shift-subtract division. Each divisor doubling costs 5 cycles and each quotient bit 8,
                         // so a quotient of k+1 bits costs 13 cycles per bit, the firmware's measured rate.
    a.emit(0xe1a0_2001); // MOV r2,r1 (shifted divisor; return_results restores r2)
    a.label("div_double");
    a.emit(0xe152_00a0); // CMP r2,r0,LSR #1
    a.emit(0x91a0_2082); // MOVLS r2,r2,LSL #1
    a.branch(9, "div_double"); // BLS
    a.emit(0xe3a0_3000); // MOV r3,#0 (quotient magnitude, also the r3 output)
    a.label("div_bit");
    a.emit(0xe150_0002); // CMP r0,r2
    a.emit(0x2040_0002); // SUBCS r0,r0,r2 (remainder magnitude)
    a.emit(0xe0a3_3003); // ADC r3,r3,r3 (quotient * 2 + comparison carry)
    a.emit(0xe1a0_20a2); // MOV r2,r2,LSR #1
    a.emit(0xe152_0001); // CMP r2,r1
    a.branch(2, "div_bit"); // BHS
    a.emit(0xe1a0_1000); // MOV r1,r0
    a.emit(0xe31c_0001); // TST r12,#1
    a.emit(0x1261_1000); // RSBNE r1,r1,#0 (remainder follows numerator sign)
    a.emit(0xe35c_0000); // CMP r12,#0
    a.emit(0x51a0_0003); // MOVPL r0,r3
    a.emit(0x4263_0000); // RSBMI r0,r3,#0 (signed quotient)
    a.branch(14, "return_results");
    a.label("div_zero");
    // Hardware returns from a zero divisor only for numerators -1, 0, and 1; other numerators
    // hang the firmware, which stays a diagnostic here.
    a.emit(0xe280_2001); // ADD r2,r0,#1
    a.emit(0xe352_0002); // CMP r2,#2
    a.branch(8, "invalid_argument"); // BHI
    a.emit(0xe350_0000); // CMP r0,#0
    a.emit(0xa3a0_2001); // MOVGE r2,#1
    a.emit(0xb3e0_2000); // MVNLT r2,#0
    a.emit(0xe58d_2004); // STR r2,[sp,#4] (saved r0 = sign, or 1)
    a.emit(0xe58d_0008); // STR r0,[sp,#8] (saved r1 = numerator)
    a.emit(0xe3a0_2001); // MOV r2,#1
    a.emit(0xe58d_2010); // STR r2,[sp,#16] (saved r3)
    a.branch(14, "return");

    // Newton iteration from a power-of-two estimate. Its loops cost 6 cycles per scaling step,
    // 6 per refinement, 5 per divisor raise, and 8 per quotient bit, the firmware's measured
    // rates; the fixed parts are padded to the measured totals.
    a.label("sqrt");
    a.emit(0xe350_0000); // CMP r0,#0
    a.branch(0, "sqrt_zero");
    pad(a, SQRT_PAD);
    a.emit(0xe3a0_1001); // MOV r1,#1 (estimate)
    a.emit(0xe1a0_2000); // MOV r2,r0
    a.label("sqrt_scale");
    a.emit(0xe151_0002); // CMP r1,r2
    a.emit(0x31a0_20a2); // MOVCC r2,r2,LSR #1
    a.emit(0x31a0_1081); // MOVCC r1,r1,LSL #1
    a.branch(3, "sqrt_scale"); // BCC
    a.label("sqrt_refine");
    // Divide the input by the estimate: r2 remainder, r3 quotient, r12 shifted divisor.
    a.emit(0xe1a0_2000); // MOV r2,r0
    a.emit(0xe3a0_3000); // MOV r3,#0
    a.emit(0xe1a0_c001); // MOV r12,r1
    a.label("sqrt_raise");
    a.emit(0xe15c_00a2); // CMP r12,r2,LSR #1
    a.emit(0x91a0_c08c); // MOVLS r12,r12,LSL #1
    a.branch(3, "sqrt_raise"); // BCC
    a.label("sqrt_bit");
    a.emit(0xe152_000c); // CMP r2,r12
    a.emit(0x2042_200c); // SUBCS r2,r2,r12
    a.emit(0xe0a3_3003); // ADC r3,r3,r3
    a.emit(0xe15c_0001); // CMP r12,r1
    a.emit(0x11a0_c0ac); // MOVNE r12,r12,LSR #1
    a.branch(1, "sqrt_bit"); // BNE
                             // Average the estimate and quotient; stop once it no longer decreases.
    a.emit(0xe081_3003); // ADD r3,r1,r3
    a.emit(0xe1a0_30a3); // MOV r3,r3,LSR #1
    a.emit(0xe153_0001); // CMP r3,r1
    a.emit(0x31a0_1003); // MOVCC r1,r3
    a.branch(3, "sqrt_refine"); // BCC
    a.emit(0xe58d_1004); // STR r1,[sp,#4] (saved r0 = floor(sqrt(input)))
    a.branch(14, "return");
    a.label("sqrt_zero");
    pad(a, SQRT_ZERO_PAD);
    a.branch(14, "return"); // The saved r0 already holds the zero result.
}

const DIV_PAD: usize = 0;
const SQRT_PAD: usize = 3;
const SQRT_ZERO_PAD: usize = 45;
