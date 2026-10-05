use super::Flags;

#[derive(Clone, Copy, Debug)]
pub(super) enum Shift {
    Lsl,
    Lsr,
    Asr,
    Ror,
}

impl Shift {
    pub(super) fn decode(bits: u32) -> Self {
        match bits & 3 {
            0 => Self::Lsl,
            1 => Self::Lsr,
            2 => Self::Asr,
            _ => Self::Ror,
        }
    }
}

/// ARM barrel shifter. Register shifts use only the low eight amount bits.
pub(super) fn shift(
    value: u32,
    kind: Shift,
    amount: u32,
    carry: bool,
    by_register: bool,
) -> (u32, bool) {
    let mut amount = if by_register { amount & 0xff } else { amount };
    if amount == 0 {
        if by_register || matches!(kind, Shift::Lsl) {
            return (value, carry);
        }
        if matches!(kind, Shift::Ror) {
            return ((u32::from(carry) << 31) | (value >> 1), value & 1 != 0); // RRX
        }
        amount = 32; // Immediate LSR/ASR #0 encodes a shift by 32.
    }
    match kind {
        Shift::Lsl => match amount {
            1..=31 => (value << amount, (value >> (32 - amount)) & 1 != 0),
            32 => (0, value & 1 != 0),
            _ => (0, false),
        },
        Shift::Lsr => match amount {
            1..=31 => (value >> amount, (value >> (amount - 1)) & 1 != 0),
            32 => (0, value >> 31 != 0),
            _ => (0, false),
        },
        Shift::Asr => {
            let sign = value >> 31 != 0;
            if amount >= 32 {
                (if sign { u32::MAX } else { 0 }, sign)
            } else {
                (
                    ((value as i32) >> amount) as u32,
                    (value >> (amount - 1)) & 1 != 0,
                )
            }
        }
        Shift::Ror => {
            let result = value.rotate_right(amount);
            (result, result >> 31 != 0)
        }
    }
}

/// Addition is also used for subtraction: a - b = a + !b + 1.
fn add_with_carry(a: u32, b: u32, carry: bool) -> (u32, bool, bool) {
    let wide = u64::from(a) + u64::from(b) + u64::from(carry);
    let result = wide as u32;
    let overflow = (!(a ^ b) & (a ^ result)) >> 31 != 0;
    (result, wide > u64::from(u32::MAX), overflow)
}

pub(super) fn execute(
    opcode: u32,
    a: u32,
    b: u32,
    shifter_carry: bool,
    previous: Flags,
) -> (u32, Flags) {
    let (result, carry, overflow) = match opcode {
        0x0 | 0x8 => (a & b, shifter_carry, previous.overflow),
        0x1 | 0x9 => (a ^ b, shifter_carry, previous.overflow),
        0x2 | 0xa => add_with_carry(a, !b, true),
        0x3 => add_with_carry(b, !a, true),
        0x4 | 0xb => add_with_carry(a, b, false),
        0x5 => add_with_carry(a, b, previous.carry),
        0x6 => add_with_carry(a, !b, previous.carry),
        0x7 => add_with_carry(b, !a, previous.carry),
        0xc => (a | b, shifter_carry, previous.overflow),
        0xd => (b, shifter_carry, previous.overflow),
        0xe => (a & !b, shifter_carry, previous.overflow),
        0xf => (!b, shifter_carry, previous.overflow),
        _ => unreachable!("opcode comes from a four-bit instruction field"),
    };
    (
        result,
        Flags {
            negative: result >> 31 != 0,
            zero: result == 0,
            carry,
            overflow,
        },
    )
}
