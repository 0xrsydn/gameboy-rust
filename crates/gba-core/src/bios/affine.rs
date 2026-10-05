//! Original ARM affine-matrix services with a mathematically generated sine table.
//! Scaling uses signed 8.8 values and the upper angle byte. Matrix intermediates
//! narrow to signed halfwords before background-origin calculation.

use super::ArmImage;

pub(super) const TABLE_ADDRESS: u32 = 0x3e00;
const UNIT: i128 = 1i128 << 48;
// floor(pi * 2^48), not data extracted from a firmware image.
const PI: i128 = 884_279_719_003_555;

const fn quarter_sine(index: usize) -> i16 {
    if index == 64 {
        return 16384;
    }
    let x = PI * index as i128 / 128;
    let mut term = x;
    let mut sum = x;
    let mut n = 1;
    while n <= 10 {
        term = -((term * x / UNIT) * x / UNIT) / ((2 * n) * (2 * n + 1));
        sum += term;
        n += 1;
    }
    (sum * 16384 / UNIT) as i16
}

const fn sine_table() -> [i16; 256] {
    let mut table = [0; 256];
    let mut index = 0;
    while index < 256 {
        let phase = index & 63;
        table[index] = match index / 64 {
            0 => quarter_sine(phase),
            1 => quarter_sine(64 - phase),
            2 => -quarter_sine(phase),
            _ => -quarter_sine(64 - phase),
        };
        index += 1;
    }
    table
}

const SINE: [i16; 256] = sine_table();

pub(super) fn write_table(image: &mut [u8]) {
    for (index, value) in SINE.iter().enumerate() {
        let address = TABLE_ADDRESS as usize + index * 2;
        image[address..address + 2].copy_from_slice(&value.to_le_bytes());
    }
}

pub(super) fn emit(a: &mut ArmImage) {
    a.label("bg_affine");
    entry(a, true);
    a.label("bg_affine_record");
    a.emit(0xe1d0_40fc); // LDRSH r4,[r0,#12] (SX)
    a.emit(0xe1d0_50fe); // LDRSH r5,[r0,#14] (SY)
    a.emit(0xe1d0_61b0); // LDRH r6,[r0,#16] (angle)
    a.call("affine_matrix");
    a.emit(0xe590_8000); // LDR r8,[r0] (texture center X, signed 24.8)
    a.emit(0xe590_9004); // LDR r9,[r0,#4] (texture center Y)
    a.emit(0xe1d0_a0f8); // LDRSH r10,[r0,#8] (display center X)
    a.emit(0xe1d0_b0fa); // LDRSH r11,[r0,#10] (display center Y)
    a.emit(0xe00c_0a94); // MUL r12,r4,r10
    a.emit(0xe048_800c); // SUB r8,r8,r12
    a.emit(0xe00c_0b95); // MUL r12,r5,r11
    a.emit(0xe088_800c); // ADD r8,r8,r12 (X = centerX - PA*displayX + SXsin*displayY)
    a.emit(0xe00c_0a96); // MUL r12,r6,r10
    a.emit(0xe049_900c); // SUB r9,r9,r12
    a.emit(0xe00c_0b97); // MUL r12,r7,r11
    a.emit(0xe049_900c); // SUB r9,r9,r12 (Y = centerY - PC*displayX - PD*displayY)
    a.emit(0xe265_5000); // RSB r5,r5,#0 (PB: negate after signed shift/narrowing)
    a.emit(0xe1c1_40b0); // STRH r4,[r1]
    a.emit(0xe1c1_50b2); // STRH r5,[r1,#2]
    a.emit(0xe1c1_60b4); // STRH r6,[r1,#4]
    a.emit(0xe1c1_70b6); // STRH r7,[r1,#6]
    a.emit(0xe581_8008); // STR r8,[r1,#8]
    a.emit(0xe581_900c); // STR r9,[r1,#12]
    a.emit(0xe280_0014); // ADD r0,r0,#20 (skip trailing two-byte source padding)
    a.emit(0xe281_1010); // ADD r1,r1,#16
    a.emit(0xe252_2001); // SUBS r2,r2,#1
    a.branch(1, "bg_affine_record");
    a.branch(14, "affine_done");

    a.label("obj_affine");
    entry(a, false);
    a.label("obj_affine_record");
    a.emit(0xe1d0_40f0); // LDRSH r4,[r0] (SX)
    a.emit(0xe1d0_50f2); // LDRSH r5,[r0,#2] (SY)
    a.emit(0xe1d0_60b4); // LDRH r6,[r0,#4] (angle)
    a.call("affine_matrix");
    a.emit(0xe265_5000); // RSB r5,r5,#0 (PB)
    for register in 4..=7 {
        a.emit(0xe1c1_00b0 | register << 12); // STRH rN,[r1]
        a.emit(0xe081_1003); // ADD r1,r1,r3 (caller-supplied coefficient stride)
    }
    a.emit(0xe280_0008); // ADD r0,r0,#8 (skip trailing two-byte padding)
    a.emit(0xe252_2001); // SUBS r2,r2,#1
    a.branch(1, "obj_affine_record");
    a.label("affine_done");
    a.emit(0xe8bd_0ff0); // LDMIA sp!,{r4-r11}
    a.branch(14, "return");

    // Leaf kernel preserves argument pointers/count/stride. r4-r7 return PA,
    // pre-negation SXsin, PC, PD. r8-r11 are scratch; no additional stack use.
    a.label("affine_matrix");
    a.emit(0xe1a0_6426); // MOV r6,r6,LSR #8 (discard fractional angle byte)
    a.literal(10, TABLE_ADDRESS);
    a.emit(0xe08a_b086); // ADD r11,r10,r6,LSL #1
    a.emit(0xe1db_80f0); // LDRSH r8,[r11] (sine in signed 2.14)
    a.emit(0xe286_6040); // ADD r6,r6,#64
    a.emit(0xe206_60ff); // AND r6,r6,#255
    a.emit(0xe08a_b086); // ADD r11,r10,r6,LSL #1
    a.emit(0xe1db_90f0); // LDRSH r9,[r11] (cosine)
    a.emit(0xe006_0895); // MUL r6,r5,r8 (SYsin)
    a.emit(0xe007_0995); // MUL r7,r5,r9 (SYcos)
    a.emit(0xe005_0894); // MUL r5,r4,r8 (SXsin)
    a.emit(0xe008_0994); // MUL r8,r4,r9 (SXcos)
    a.emit(0xe1a0_4748); // MOV r4,r8,ASR #14
    for register in 5..=7 {
        a.emit(0xe1a0_0740 | register << 12 | register); // MOV rN,rN,ASR #14
    }
    for register in 4..=7 {
        a.emit(0xe1a0_0800 | register << 12 | register); // MOV rN,rN,LSL #16
        a.emit(0xe1a0_0840 | register << 12 | register); // MOV rN,rN,ASR #16
    }
    a.emit(0xe12f_ff1e); // BX lr
}

fn entry(a: &mut ArmImage, background: bool) {
    a.emit(0xe352_0000); // CMP r2,#0
    a.branch(0, "return"); // Zero count does not validate or access buffers.
    if !background {
        // SWI dispatch uses r3 for caller status. Recover the fourth argument
        // from the common frame before adding our local register save.
        a.emit(0xe59d_3010); // LDR r3,[sp,#16] (original coefficient stride)
    }
    a.emit(0xe92d_0ff0); // STMDB sp!,{r4-r11} (60 bytes total SVC stack use)
    a.emit(0xe180_c001); // ORR r12,r0,r1
    a.emit(if background { 0xe31c_0003 } else { 0xe31c_0001 }); // TST alignment
    a.branch(1, "invalid_argument");
    a.emit(0xe350_0402); // CMP r0,#0x02000000
    a.branch(3, "invalid_argument");
    if !background {
        a.emit(0xe313_0001); // TST r3,#1 (even coefficient stride)
        a.branch(1, "invalid_argument");
        a.emit(0xe353_0002); // CMP r3,#2
        a.branch(3, "invalid_argument");
        a.emit(0xe313_0103); // TST r3,#0xc0000000 (4*stride must not wrap)
        a.branch(1, "invalid_argument");
    }
    a.emit(if background { 0xe3a0_6014 } else { 0xe3a0_6008 }); // MOV r6,#source stride
    check_span(a, 0);
    a.emit(if background { 0xe3a0_6010 } else { 0xe1a0_6103 }); // MOV r6,#16 / r3,LSL #2
    check_span(a, 1);
}

fn check_span(a: &mut ArmImage, pointer: u32) {
    a.emit(0xe085_4692); // UMULL r4,r5,r2,r6 (full unsigned count*stride)
    a.emit(0xe355_0000); // CMP r5,#0
    a.branch(1, "invalid_argument");
    a.emit(0xe090_4004 | pointer << 16); // ADDS r4,rPointer,r4
    a.branch(2, "invalid_argument");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_table_matches_independent_trigonometry_for_every_phase() {
        for (phase, actual) in SINE.iter().enumerate() {
            let angle = phase as f64 * std::f64::consts::TAU / 256.0;
            assert_eq!(
                *actual,
                (angle.sin() * 16384.0).trunc() as i16,
                "phase={phase}"
            );
        }
        assert_eq!(
            [SINE[0], SINE[64], SINE[128], SINE[192]],
            [0, 16384, 0, -16384]
        );
    }
}
