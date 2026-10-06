//! Original ARM probes compared with independently published read observations.
//! See docs/research/prefetch-cancellation.md for provenance and measurement limits.
use gba_demos::prefetch_probe::{measure, ProbeError, PUBLISHED_READ_DELTAS, WAITCNT_SETTINGS};

#[test]
fn instruction_boundary_deltas_match_published_read_observations() {
    for (index, expected) in PUBLISHED_READ_DELTAS.into_iter().enumerate() {
        for (waitcnt, expected) in WAITCNT_SETTINGS.into_iter().zip(expected) {
            let idle = index as u8 + 1;
            let control = measure(waitcnt, idle, false).unwrap();
            let read = measure(waitcnt, idle, true).unwrap();
            assert_eq!(
                read.boundary_cycles - control.boundary_cycles,
                u64::from(expected),
                "idle={idle}, WAITCNT={waitcnt:#06x}"
            );
            for sample in [control, read] {
                assert_eq!(sample.multiply_internal_cycles, u32::from(idle));
                assert_eq!(sample.completion_pc, 0x0800_0100 + sample.steps as u32 * 4);
                assert_eq!(sample.sample_timing.data_cycles, 1);
                assert_eq!(sample.sample_timing.internal_cycles, 1);
            }
            assert_eq!(read.steps, control.steps + 1);
            assert_eq!(control.rom_read_timing, None);
            assert_eq!(read.rom_read_timing.unwrap().internal_cycles, 1);
        }
    }
}

#[test]
fn observed_extra_cycle_is_in_rom_data_cancellation_not_internal_work() {
    for idle in 1..=8 {
        for (index, waitcnt) in WAITCNT_SETTINGS.into_iter().enumerate() {
            let sample = measure(waitcnt, idle, true).unwrap();
            let data = sample.rom_read_timing.unwrap();
            let raw_word = [8, 7, 7, 6][index];
            let stall = u32::from(index >= 2 && matches!(idle, 4 | 7));
            assert_eq!(data.data_cycles, raw_word + stall);
            assert_eq!(data.internal_cycles, 1);
        }
    }
}

#[test]
fn unadjusted_timer_samples_match_published_observations_at_the_bus_access() {
    // Keep the original program and published values unchanged. No result correction.
    for (index, expected) in PUBLISHED_READ_DELTAS.into_iter().enumerate() {
        for (waitcnt, expected) in WAITCNT_SETTINGS.into_iter().zip(expected) {
            let control = measure(waitcnt, index as u8 + 1, false).unwrap();
            let read = measure(waitcnt, index as u8 + 1, true).unwrap();
            let sampled = i64::from(read.timer_sample) - i64::from(control.timer_sample);
            let boundary = read.boundary_cycles as i64 - control.boundary_cycles as i64;
            assert_eq!(sampled, i64::from(expected));
            assert_eq!(boundary, sampled);
            assert!(read.sample_timing.code_cycles > control.sample_timing.code_cycles);
        }
    }
}

#[test]
fn disabling_prefetch_removes_the_cancellation_stall_and_endpoint_fetch_difference() {
    for idle in 1..=8 {
        for (index, waitcnt) in WAITCNT_SETTINGS.into_iter().enumerate() {
            let waitcnt = waitcnt & !0x4000;
            let control = measure(waitcnt, idle, false).unwrap();
            let read = measure(waitcnt, idle, true).unwrap();
            // Extra LDR: N word code + N word data + one internal cycle.
            let expected = [17, 15, 15, 13][index];
            assert_eq!(read.boundary_cycles - control.boundary_cycles, expected);
            assert_eq!(
                u64::from(read.timer_sample - control.timer_sample),
                expected
            );
            assert_eq!(read.sample_timing, control.sample_timing);
            assert_eq!(
                read.rom_read_timing.unwrap().data_cycles,
                [8, 7, 7, 6][index]
            );
        }
    }
}

#[test]
fn probe_rejects_unbounded_idle_requests_and_repeats_without_shared_state() {
    for idle in [0, 9, u8::MAX] {
        for with_read in [false, true] {
            assert!(
                matches!(measure(0x4000, idle, with_read), Err(ProbeError::IdleCycles(value)) if value == idle)
            );
        }
    }
    let first = measure(0x4010, 4, true).unwrap();
    measure(0x4004, 8, false).unwrap();
    assert_eq!(measure(0x4010, 4, true).unwrap(), first);
}
