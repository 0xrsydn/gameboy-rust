//! The within-line shortcut in `Io::advance` against the general event path.
use super::*;
use crate::display::{CYCLES_PER_LINE, HBLANK_START};

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }
}

/// Arbitrary display, window, mosaic, affine, and timer register contents.
fn scramble(io: &mut Io, random: &mut Lcg) {
    for address in 0x0400_0000..=0x0400_0057 {
        io.write8(address, random.next() as u8);
    }
    for address in TIMER_BASE..=0x0400_010f {
        io.write8(address, random.next() as u8);
    }
    io.write8(IE, random.next() as u8);
}

#[test]
fn steps_inside_a_line_match_batches_that_always_reach_a_display_edge() {
    for seed in 0..12 {
        let mut random = Lcg(seed);
        let mut stepped = Io::default();
        let mut batched = Io::default();
        for line in 0..3 * 228 {
            if line % 5 == 0 {
                let mut writes = Lcg(random.next().into());
                scramble(&mut stepped, &mut writes.clone_state());
                scramble(&mut batched, &mut writes);
            }
            // Instruction-sized steps mostly take the shortcut.
            let mut remaining = CYCLES_PER_LINE;
            while remaining != 0 {
                let cycles = (1 + random.next() % 24).min(remaining);
                stepped.advance(cycles, true);
                remaining -= cycles;
            }
            // Each of these batches ends on HBlank or on a line start, so neither can.
            batched.advance(HBLANK_START, true);
            batched.advance(CYCLES_PER_LINE - HBLANK_START, true);
            assert_eq!(stepped, batched, "seed {seed} line {line}");
        }
    }
}

impl Lcg {
    fn clone_state(&self) -> Self {
        Self(self.0)
    }
}
