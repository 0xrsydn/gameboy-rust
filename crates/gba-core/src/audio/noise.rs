//! Nominal GBA channel 4 polynomial counter. No random numbers or host clocks.
use super::modulation::Modulation;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Noise {
    pub(super) modulation: Modulation,
    polynomial: u8,
    lfsr: u16,
    remaining: u32,
    high: bool,
}

impl Noise {
    pub(super) fn read(&self, offset: u32) -> u8 {
        match offset {
            1 => self.modulation.envelope,
            4 => self.polynomial,
            5 => u8::from(self.modulation.length_enabled) << 6,
            _ => 0,
        }
    }

    fn short(&self) -> bool {
        self.polynomial & 8 != 0
    }

    fn period(&self) -> u32 {
        let ratio = u32::from(self.polynomial & 7);
        let base = if ratio == 0 { 32 } else { 64 * ratio };
        base << (self.polynomial >> 4)
    }

    pub(super) fn write(&mut self, offset: u32, value: u8, next_step: u8) {
        match offset {
            0 => self.modulation.write_length(value),
            1 => self.modulation.write_envelope(value),
            4 => self.polynomial = value, // Preserve counter and remaining interval on live writes.
            5 => {
                if self.modulation.write_control(value, next_step) {
                    self.lfsr = if self.short() { 0x40 } else { 0x4000 };
                    self.remaining = self.period();
                    self.high = false;
                }
            }
            _ => {}
        }
    }

    pub(super) fn advance(&mut self, cycles: u32) {
        if !self.modulation.active || cycles == 0 {
            return;
        }
        if cycles < self.remaining {
            self.remaining -= cycles;
            return;
        }
        let rest = cycles - self.remaining;
        let period = self.period();
        let edges = 1 + rest / period;
        // The output is the carry of the final shift, not the final register's low bit.
        let before_last = jump(self.lfsr, edges - 1, self.short());
        self.high = before_last & 1 != 0;
        self.lfsr = shift(before_last, self.short());
        self.remaining = period - rest % period;
    }

    pub(super) fn clock(&mut self, step: u8) {
        self.modulation.clock(step);
    }

    pub(super) fn sample(&self) -> i16 {
        if !self.modulation.active {
            return 0;
        }
        if self.high {
            i16::from(self.modulation.volume)
        } else {
            -i16::from(self.modulation.volume)
        }
    }
}

const fn shift(state: u16, short: bool) -> u16 {
    (state >> 1)
        ^ if state & 1 != 0 {
            if short {
                0x60
            } else {
                0x6000
            }
        } else {
            0
        }
}

// Each column maps one input bit. Squaring the linear map doubles its step count.
// This also handles transient upper bits after a live width change; reducing by
// the 127-step short-mode period would incorrectly discard those transient bits.
type Transform = [u16; 15];
const JUMPS: [[Transform; 32]; 2] = [powers(false), powers(true)];

const fn apply(map: &Transform, mut state: u16) -> u16 {
    let mut result = 0;
    while state != 0 {
        result ^= map[state.trailing_zeros() as usize];
        state &= state - 1;
    }
    result
}

const fn powers(short: bool) -> [Transform; 32] {
    let mut maps = [[0; 15]; 32];
    let mut bit = 0;
    while bit < 15 {
        maps[0][bit] = shift(1 << bit, short);
        bit += 1;
    }
    let mut power = 1;
    while power < 32 {
        bit = 0;
        while bit < 15 {
            maps[power][bit] = apply(&maps[power - 1], maps[power - 1][bit]);
            bit += 1;
        }
        power += 1;
    }
    maps
}

fn jump(mut state: u16, mut count: u32, short: bool) -> u16 {
    let maps = &JUMPS[usize::from(short)];
    let mut power = 0;
    while count != 0 {
        if count & 1 != 0 {
            state = apply(&maps[power], state);
        }
        count >>= 1;
        power += 1;
    }
    state
}

#[cfg(test)]
mod tests;
