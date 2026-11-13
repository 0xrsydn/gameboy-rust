//! ARM7TDMI multiplier early termination and its carry flag.
//!
//! The ARM documentation calls the carry flag after a multiply "meaningless", but hardware sets it
//! deterministically from the Booth multiplier's carry-save adders. This is an original Rust
//! implementation of the compact model by zaydlang and calc84maniac (2024, zlib license), which
//! was derived from hardware fuzzing; see `docs/hardware/cpu.md` and `docs/references.md`.

/// Multiplier iterations of 8 bits each (1–4). Iteration stops once the remaining upper multiplier
/// bits are all zero, or, for signed forms, all one. `MUL`/`MLA` count as signed.
pub(super) fn iterations(multiplier: u32, signed: bool) -> u32 {
    for bytes in 1..=3 {
        let mask = u32::MAX << (bytes * 8);
        let high = multiplier & mask;
        if high == 0 || signed && high == mask {
            return bytes;
        }
    }
    4
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Form {
    /// `MUL`/`MLA` and Thumb `MUL`.
    Short,
    /// `UMULL`/`UMLAL`.
    UnsignedLong,
    /// `SMULL`/`SMLAL`.
    SignedLong,
}

/// The carry flag after a flag-setting multiply. `accumulate` is 0 without accumulation; for short
/// forms only its low word is used.
pub(super) fn carry(form: Form, multiplicand: u32, multiplier: u32, accumulate: u64) -> bool {
    let signed = form != Form::UnsignedLong;
    if iterations(multiplier, signed) < 4 {
        return early_carry(multiplicand, multiplier, accumulate as u32);
    }
    match form {
        // The final Booth addend is negative, injecting its carry, only for top bits `10`.
        Form::Short => multiplier >> 30 == 2,
        Form::UnsignedLong => {
            long_carry(multiplicand, multiplier, (accumulate >> 32) as u32, false)
        }
        Form::SignedLong => long_carry(multiplicand, multiplier, (accumulate >> 32) as u32, true),
    }
}

fn sign_extend_from(value: u32, shift: u32) -> u32 {
    ((value << shift) as i32 >> shift) as u32
}

/// After early termination, the carry is bit 31 of the carry-save adders' final carry word.
/// Only the low 32 bits matter, so the adders run on 32-bit values.
fn early_carry(multiplicand: u32, multiplier: u32, mut accumulate: u32) -> bool {
    // An odd multiplicand makes Booth negation invert the upper bits; bit 0 cannot reach bit 31.
    let multiplicand = multiplicand | 1;
    let mut booth = sign_extend_from(multiplier, 31);
    let mut carry = multiplicand.wrapping_mul(booth);
    let mut sum = carry.wrapping_add(accumulate);
    let mut shift = 29;
    loop {
        // Each iteration takes 8 multiplier bits as four Booth digits.
        for _ in 0..4 {
            let next = sign_extend_from(multiplier, shift);
            let addend = multiplicand.wrapping_mul(next.wrapping_sub(booth));
            booth = next;
            accumulate ^= carry ^ addend;
            sum = sum.wrapping_add(addend);
            carry = sum.wrapping_sub(accumulate);
            // Early termination ends within three iterations, before the shift underflows.
            shift = shift.saturating_sub(2);
        }
        if booth == multiplier {
            return carry >> 31 != 0;
        }
    }
}

/// After all four iterations of a long multiply, the carry is bit 63 of the final carry word.
/// Only the last three Booth digits can reach it, so the inputs are scaled down to those bits.
fn long_carry(multiplicand: u32, multiplier: u32, accumulate_high: u32, signed: bool) -> bool {
    let (multiplicand, multiplier) = if signed {
        (
            (multiplicand as i32 >> 6) as u32,
            (multiplier as i32 >> 26) as u32,
        )
    } else {
        (multiplicand >> 6, multiplier >> 26)
    };
    let multiplicand = multiplicand | 1;

    // Bits 63-60 of the sign-extension correction, with the carry correction folded in.
    let carry_correction = !accumulate_high & 0x2000_0000;
    let mut accumulate = accumulate_high.wrapping_sub(0x0800_0000);

    let booth0 = sign_extend_from(multiplier, 27);
    let booth1 = sign_extend_from(multiplier, 29);
    let booth2 = sign_extend_from(multiplier, 31);
    let factor0 = multiplier.wrapping_sub(booth0);
    let factor1 = booth0.wrapping_sub(booth1);
    let factor2 = booth1.wrapping_sub(booth2);

    // Third-last addend: its sign finishes correction bits 61-60.
    let addend = multiplicand.wrapping_mul(factor2);
    accumulate = accumulate.wrapping_sub(addend & 0x1000_0000);
    // Second-last addend: its sign finishes correction bits 63-62.
    let addend = multiplicand.wrapping_mul(factor1);
    accumulate = accumulate.wrapping_sub(addend & 0x4000_0000);
    // The carry-save carry out of bit 61 propagates into bit 62.
    let mut sum = accumulate.wrapping_add(addend & 0x2000_0000);
    accumulate = accumulate.wrapping_sub(carry_correction);
    // Last addend, added at bit 62.
    let addend = multiplicand.wrapping_mul(factor0);
    sum = sum.wrapping_add(addend & 0x4000_0000);
    (sum ^ accumulate) >> 31 != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iterations_stop_on_zero_or_signed_one_bytes() {
        assert_eq!(iterations(0xff, false), 1);
        assert_eq!(iterations(0xffff_ff80, true), 1);
        assert_eq!(iterations(0xffff_ff80, false), 4);
        assert_eq!(iterations(0x0001_0000, true), 3);
        assert_eq!(iterations(0x8000_0000, true), 4);
    }

    // Hardware results from the mGBA test suite's multiply-long tests that set carry:
    // (Rm, Rs) with UMULLS and SMULLS.
    #[test]
    fn long_multiplies_match_hardware_carry_results() {
        for (rm, rs, unsigned, signed) in [
            (0xffff_ffff, 0xffff_ffff, true, false),
            (0x7fff_ffff, 0xffff_ffff, true, false),
            (0xffff_ffff, 0x7fff_ffff, true, true),
            (0x8000_0000, 0x7fff_ffff, true, true),
            (1, 1, false, false),
            (0, 0, false, false),
        ] {
            assert_eq!(
                carry(Form::UnsignedLong, rm, rs, 0),
                unsigned,
                "{rm:#x} {rs:#x}"
            );
            assert_eq!(
                carry(Form::SignedLong, rm, rs, 0),
                signed,
                "{rm:#x} {rs:#x}"
            );
        }
    }

    // Regression vectors from a prototype of this model that matched all 22,723 flag-setting
    // multiplies in the public SingleStepTests ARM7TDMI vectors (generated by NanoBoyAdvance).
    #[test]
    fn short_and_accumulating_forms_follow_the_booth_model() {
        assert!(carry(Form::Short, 0, 0x8000_0000, 0));
        assert!(!carry(Form::Short, 0, 0xc000_0000, 0));
        assert!(!carry(Form::Short, 0, 0x4000_0000, 0));
        for (form, rm, rs, accumulate, expected) in [
            (Form::Short, 0x1234_5678, 0x0000_00ff, 0, false),
            (Form::Short, 0xffff_ffff, 0x0000_0002, 0, false),
            (Form::Short, 0x8000_0001, 0x0000_7fff, 0x8000_0000, false),
            (Form::Short, 0x89ab_cdef, 0x00ff_00ff, 0x1357_9bdf, true),
            (Form::UnsignedLong, 0x8000_0000, 0x0000_0003, 0, false),
            (Form::UnsignedLong, 0xfedc_ba98, 0x0000_1234, 0, false),
            (Form::UnsignedLong, u32::MAX, u32::MAX, u64::MAX, true),
            (
                Form::SignedLong,
                0xdead_beef,
                0x0123_4567,
                0xffff_ffff_0000_0000,
                false,
            ),
            (Form::SignedLong, 0x7fff_ffff, 0xffff_ff80, 0, false),
            (
                Form::SignedLong,
                0x4000_0000,
                0x8000_0000,
                0x1234_5678_9abc_def0,
                true,
            ),
        ] {
            assert_eq!(
                carry(form, rm, rs, accumulate),
                expected,
                "{form:?} {rm:#x} {rs:#x} {accumulate:#x}"
            );
        }
    }

    /// Local check against the public SingleStepTests ARM7TDMI vectors (MIT, generated by
    /// NanoBoyAdvance). They are not committed; put `arm_mul_mla.json.bin`,
    /// `arm_mull_mlal.json.bin`, and `thumb_data_proc.json.bin` in `roms/research/singlestep/`
    /// and run `cargo test -p gba-core multiply -- --ignored`.
    #[test]
    #[ignore = "needs local SingleStepTests vectors"]
    fn matches_single_step_vectors() {
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../roms/research/singlestep");
        let mut checked = 0;
        for name in ["arm_mul_mla", "arm_mull_mlal", "thumb_data_proc"] {
            let Ok(buffer) = std::fs::read(directory.join(format!("{name}.json.bin"))) else {
                continue;
            };
            let word = |at: usize| u32::from_le_bytes(buffer[at..at + 4].try_into().unwrap());
            assert_eq!(word(0), 0xd33d_bae0);
            let mut at = 8;
            for _ in 0..word(4) {
                let size = word(at) as usize;
                // Test: size, initial state, final state, transactions, opcode.
                let initial = at + 4;
                let last = initial + word(initial) as usize;
                let transactions = last + word(last) as usize;
                let opcode = word(transactions + word(transactions) as usize + 8);
                let state = |base: usize, index: usize| word(base + 8 + index * 4);
                let cpsr = state(initial, 31);
                let register = |index: u32| {
                    let index = index as usize;
                    let banked = match (cpsr & 0x1f, index) {
                        (0x11, 8..=14) => Some(16 + index - 8),
                        (0x13, 13..=14) => Some(23 + index - 13),
                        (0x17, 13..=14) => Some(25 + index - 13),
                        (0x12, 13..=14) => Some(27 + index - 13),
                        (0x1b, 13..=14) => Some(29 + index - 13),
                        _ => None,
                    };
                    state(initial, banked.unwrap_or(index))
                };
                let expected = state(last, 31) >> 29 & 1 != 0;
                let actual = if name == "thumb_data_proc" {
                    if opcode >> 6 & 0x3ff != 0x10d {
                        at += size;
                        continue;
                    }
                    carry(
                        Form::Short,
                        register(opcode >> 3 & 7),
                        register(opcode & 7),
                        0,
                    )
                } else {
                    let fields = [0, 8, 12, 16].map(|shift| opcode >> shift & 15);
                    let flags = (cpsr >> 28) as u8;
                    if opcode & (1 << 20) == 0
                        || fields.contains(&15)
                        || !passes(opcode >> 28, flags)
                    {
                        at += size;
                        continue;
                    }
                    let (rm, rs) = (register(fields[0]), register(fields[1]));
                    let accumulate = opcode & (1 << 21) != 0;
                    if name == "arm_mull_mlal" {
                        let form = if opcode & (1 << 22) != 0 {
                            Form::SignedLong
                        } else {
                            Form::UnsignedLong
                        };
                        let value = if accumulate {
                            u64::from(register(fields[3])) << 32 | u64::from(register(fields[2]))
                        } else {
                            0
                        };
                        carry(form, rm, rs, value)
                    } else {
                        let value = if accumulate { register(fields[2]) } else { 0 };
                        carry(Form::Short, rm, rs, u64::from(value))
                    }
                };
                assert_eq!(actual, expected, "{name} {opcode:#010x}");
                checked += 1;
                at += size;
            }
        }
        assert!(checked > 0, "no vectors in {}", directory.display());
    }

    fn passes(condition: u32, flags: u8) -> bool {
        let [n, z, c, v] = [8, 4, 2, 1].map(|bit| flags & bit != 0);
        match condition {
            0 => z,
            1 => !z,
            2 => c,
            3 => !c,
            4 => n,
            5 => !n,
            6 => v,
            7 => !v,
            8 => c && !z,
            9 => !c || z,
            10 => n == v,
            11 => n != v,
            12 => !z && n == v,
            13 => z || n != v,
            14 => true,
            _ => false,
        }
    }
}
