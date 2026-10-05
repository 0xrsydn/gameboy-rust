//! Row-at-once approximation of the one-line-ahead OBJ pipeline.
//! OAM and VRAM sample at cycle40 of the preceding line. This does not model
//! individual fetches, fetch budgets, or OAM/VRAM contention.

use super::PreparedObjects;
use crate::{display::CYCLES_PER_LINE, video::VideoError};

pub(crate) const PREPARE_CYCLE: u32 = 40;

#[derive(Default)]
pub(crate) struct SpritePipeline {
    rows: [Option<(usize, Result<PreparedObjects, VideoError>)>; 2],
    pub started: bool,
}

impl SpritePipeline {
    pub fn store(&mut self, row: usize, objects: Result<PreparedObjects, VideoError>) {
        self.rows[row % 2] = Some((row, objects));
    }

    pub fn row(&self, row: usize) -> &Result<PreparedObjects, VideoError> {
        let (prepared_row, objects) = self.rows[row % 2]
            .as_ref()
            .expect("sprite row prepared before capture");
        assert_eq!(*prepared_row, row);
        objects
    }
}

/// Only rows0..158 and227 prepare the following visible row.
pub(crate) fn target(scanline: u16) -> Option<usize> {
    match scanline {
        0..=158 => Some(usize::from(scanline) + 1),
        227 => Some(0),
        _ => None,
    }
}

pub(crate) fn next_event(scanline: u16, cycle: u16) -> u32 {
    let (line, cycle) = (u32::from(scanline), u32::from(cycle));
    if target(scanline).is_some() && cycle < PREPARE_CYCLE {
        return PREPARE_CYCLE - cycle;
    }
    let lines = if (158..227).contains(&line) {
        227 - line
    } else {
        1
    };
    lines * CYCLES_PER_LINE + PREPARE_CYCLE - cycle
}

/// Latest two events suffice when capture is disabled: fixed memory/registers
/// cannot change inside a clock batch, and only two prepared rows are retained.
/// Offsets are relative to the supplied starting phase, in chronological order.
pub(crate) fn latest_events(phase: u32, cycles: u32) -> [Option<(u32, usize)>; 2] {
    let start = u64::from(phase);
    let end = start + u64::from(cycles);
    let latest = |end: u64| {
        let mut line = end.checked_sub(u64::from(PREPARE_CYCLE))? / u64::from(CYCLES_PER_LINE);
        let row = line % 228;
        if (159..227).contains(&row) {
            line -= row - 158;
        }
        let time = line * u64::from(CYCLES_PER_LINE) + u64::from(PREPARE_CYCLE);
        (time > start).then_some((time, ((line + 1) % 228) as usize))
    };
    let Some((last, row)) = latest(end) else {
        return [None; 2];
    };
    let previous = latest(last - 1).map(|(time, row)| ((time - start) as u32, row));
    [previous, Some(((last - start) as u32, row))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::CYCLES_PER_FRAME;

    #[test]
    fn event_selection_matches_independent_line_iteration() {
        let mut seed = 0x6328_93abu32;
        for case in 0..1000 {
            let mut next = || {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                seed
            };
            let phase = next() % CYCLES_PER_FRAME;
            let cycles = if case == 0 {
                u32::MAX
            } else {
                next() % (CYCLES_PER_FRAME * 5)
            };
            let end = u64::from(phase) + u64::from(cycles);
            let mut expected = [None; 2];
            for line in u64::from(phase / CYCLES_PER_LINE)..=end / u64::from(CYCLES_PER_LINE) {
                let time = line * u64::from(CYCLES_PER_LINE) + 40;
                if time > u64::from(phase) && time <= end && (line % 228 < 159 || line % 228 == 227)
                {
                    expected[0] = expected[1];
                    expected[1] = Some((
                        (time - u64::from(phase)) as u32,
                        ((line + 1) % 228) as usize,
                    ));
                }
            }
            assert_eq!(latest_events(phase, cycles), expected);
        }
    }

    #[test]
    fn next_event_handles_visible_end_hidden_lines_and_exact_boundary() {
        for line in 0..228 {
            for cycle in [0, 39, 40, 1006, 1231] {
                let phase = u32::from(line) * CYCLES_PER_LINE + u32::from(cycle);
                let distance = next_event(line, cycle);
                assert!(distance > 0);
                assert_eq!(latest_events(phase, distance - 1), [None; 2]);
                assert!(latest_events(phase, distance)[1].is_some());
            }
        }
    }
}
