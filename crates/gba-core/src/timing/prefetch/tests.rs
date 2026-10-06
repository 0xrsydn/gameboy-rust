//! Independent queue expectations. These tests do not call bus_cycles for expected costs.
use super::*;

const PC: u32 = 0x0800_1000;
const ENABLE: u16 = 0x4000;

fn queue(waitcnt: u16) -> Prefetch {
    let mut queue = Prefetch::default();
    queue.configure(waitcnt);
    queue
}

#[test]
fn save_wait_changes_preserve_the_queue_until_a_flash_data_access_cancels_it() {
    for (wait, raw) in [5, 4, 3, 9].into_iter().enumerate() {
        let mut q = queue(ENABLE);
        assert_eq!(
            q.code(PC, AccessWidth::Halfword, AccessKind::NonSequential),
            5
        );
        q.advance(2); // One cycle remains in the nominal three-cycle ROM halfword.
        let stream = q.stream;
        q.configure(ENABLE | wait as u16);
        assert_eq!(q.stream, stream);
        assert_eq!(
            q.data(0x0e005555, AccessWidth::Byte, AccessKind::NonSequential),
            raw + 1
        );
        assert_eq!(q.stream, None);
        assert_eq!(
            q.code(PC + 2, AccessWidth::Halfword, AccessKind::NonSequential),
            5
        );
    }
}

#[test]
fn ready_and_in_progress_fetches_follow_all_rom_wait_settings_and_widths() {
    for window in 0..3 {
        for first in 0..4 {
            for second in 0..2 {
                let waitcnt =
                    ENABLE | (first << [2, 5, 8][window]) | (second << [4, 7, 10][window]);
                let n = [5, 4, 3, 9][first as usize];
                let s = if second == 1 { 2 } else { [3, 5, 9][window] };
                for width in [AccessWidth::Halfword, AccessWidth::Word] {
                    let halves = width.bytes() / 2;
                    let pc = PC + window as u32 * 0x0200_0000;
                    for idle in 0..=2 * s + 2 {
                        for kind in [AccessKind::Sequential, AccessKind::NonSequential] {
                            let mut q = queue(waitcnt);
                            assert_eq!(
                                q.code(pc, width, AccessKind::NonSequential),
                                n + (halves - 1) * s
                            );
                            q.advance(idle);
                            // A live cartridge burst ignores the CPU's N request, including after I cycles.
                            assert_eq!(
                                q.code(pc + width.bytes(), width, kind),
                                (halves * s).saturating_sub(idle).max(1),
                                "window={window} waitcnt={waitcnt:#x} width={width:?} idle={idle}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn sequential_fetches_without_spare_bus_time_keep_the_sequential_rom_cost() {
    for width in [AccessWidth::Halfword, AccessWidth::Word] {
        let mut q = queue(ENABLE);
        q.code(PC, width, AccessKind::Sequential);
        for index in 1..100 {
            assert_eq!(
                q.code(PC + index * width.bytes(), width, AccessKind::Sequential),
                if width == AccessWidth::Word { 6 } else { 3 }
            );
        }
    }
}

#[test]
fn full_queue_pauses_until_drained_then_restarts_nonsequentially() {
    for width in [AccessWidth::Halfword, AccessWidth::Word] {
        let mut q = queue(ENABLE);
        q.code(PC, width, AccessKind::Sequential);
        q.advance(25); // Eight halfwords at S=3, then one cycle observing full capacity.
        assert_eq!(q.stream.unwrap().ready, 8);
        assert!(!q.stream.unwrap().running);
        let mut huge = q;
        huge.advance(u32::MAX);
        assert_eq!(q, huge);
        let count = 16 / width.bytes();
        for index in 1..=count {
            assert_eq!(
                q.code(PC + index * width.bytes(), width, AccessKind::Sequential),
                1
            );
        }
        assert_eq!(
            q.code(
                PC + (count + 1) * width.bytes(),
                width,
                AccessKind::Sequential
            ),
            if width == AccessWidth::Word { 8 } else { 5 }
        );
        assert_eq!(
            q.code(
                PC + (count + 2) * width.bytes(),
                width,
                AccessKind::Sequential
            ),
            if width == AccessWidth::Word { 6 } else { 3 }
        );
    }
}

#[test]
fn arm_fetch_can_combine_a_ready_low_half_with_an_in_progress_high_half() {
    for (idle, cost) in [(0, 6), (1, 5), (2, 4), (3, 3), (4, 2), (5, 1), (6, 1)] {
        let mut q = queue(ENABLE);
        q.code(PC, AccessWidth::Word, AccessKind::NonSequential);
        q.advance(idle);
        assert_eq!(
            q.code(PC + 4, AccessWidth::Word, AccessKind::NonSequential),
            cost
        );
    }
    // Halfword ownership also permits a state change at the exact next queued address.
    let mut q = queue(ENABLE);
    q.code(PC + 2, AccessWidth::Halfword, AccessKind::NonSequential);
    q.advance(6);
    assert_eq!(
        q.code(PC + 4, AccessWidth::Word, AccessKind::NonSequential),
        1
    );
    assert_eq!(
        q.code(PC + 8, AccessWidth::Halfword, AccessKind::Sequential),
        2
    );
}

#[test]
fn arm_word_restarts_between_halves_after_draining_an_odd_full_queue() {
    let mut q = queue(ENABLE);
    q.code(PC, AccessWidth::Halfword, AccessKind::Sequential);
    q.advance(25);
    assert_eq!(
        q.code(PC + 2, AccessWidth::Halfword, AccessKind::Sequential),
        1
    );
    for offset in [4, 8, 12] {
        assert_eq!(
            q.code(PC + offset, AccessWidth::Word, AccessKind::Sequential),
            1
        );
    }
    // The low half is the last ready entry. The missing high half restarts with N=5.
    assert_eq!(
        q.code(PC + 16, AccessWidth::Word, AccessKind::Sequential),
        6
    );
    assert_eq!(
        q.code(PC + 20, AccessWidth::Word, AccessKind::Sequential),
        6
    );
}

#[test]
fn cancellation_stalls_only_at_the_last_cycle_of_an_active_halfword() {
    for idle in 0..=26 {
        let stall = u32::from(idle < 24 && idle % 3 == 2);
        let mut q = queue(ENABLE);
        q.code(PC, AccessWidth::Halfword, AccessKind::Sequential);
        q.advance(idle);
        let mut branch = q;
        assert_eq!(
            q.data(PC + 0x100, AccessWidth::Word, AccessKind::NonSequential),
            8 + stall
        );
        assert!(q.stream.is_none());
        assert_eq!(
            branch.code(PC + 0x100, AccessWidth::Word, AccessKind::NonSequential),
            8 + stall
        );
        assert_eq!(branch.stream.unwrap().head, PC + 0x104);
    }
}

#[test]
fn only_the_queue_head_can_hit_and_rom_data_never_consumes_opcode_entries() {
    let mut q = queue(ENABLE);
    q.code(PC, AccessWidth::Halfword, AccessKind::Sequential);
    q.advance(12);
    assert_eq!(
        q.code(PC + 6, AccessWidth::Halfword, AccessKind::NonSequential),
        5
    );
    q.advance(12);
    let head = q.stream.unwrap().head;
    assert_eq!(
        q.data(head, AccessWidth::Halfword, AccessKind::NonSequential),
        5
    );
    assert!(q.stream.is_none());
}

#[test]
fn other_memory_and_internal_work_advance_the_queue_without_arming_it() {
    let mut q = queue(ENABLE);
    assert_eq!(
        q.data(0x0200_0000, AccessWidth::Word, AccessKind::NonSequential),
        6
    );
    q.advance(10);
    assert!(q.stream.is_none());
    q.code(PC, AccessWidth::Word, AccessKind::Sequential);
    assert_eq!(
        q.data(0x0200_0000, AccessWidth::Word, AccessKind::NonSequential),
        6
    );
    assert_eq!(
        q.code(PC + 4, AccessWidth::Word, AccessKind::NonSequential),
        1
    );
    assert_eq!(
        q.code(0x0300_0000, AccessWidth::Word, AccessKind::NonSequential),
        1
    );
    q.advance(1);
    assert_eq!(
        q.code(PC + 8, AccessWidth::Halfword, AccessKind::NonSequential),
        1
    );
}

#[test]
fn page_boundaries_pause_background_work_and_demand_uses_the_new_window() {
    for (boundary, word_n, word_s) in [
        (0x0802_0000, 8, 6),
        (0x0a00_0000, 10, 10),
        (0x0c00_0000, 14, 18),
    ] {
        let mut q = queue(ENABLE);
        q.code(boundary - 8, AccessWidth::Word, AccessKind::Sequential);
        q.advance(u32::MAX);
        assert_eq!(q.stream.unwrap().ready, 2);
        assert!(!q.stream.unwrap().running);
        assert_eq!(
            q.code(boundary - 4, AccessWidth::Word, AccessKind::Sequential),
            1
        );
        assert_eq!(
            q.code(boundary, AccessWidth::Word, AccessKind::Sequential),
            word_n
        );
        assert_eq!(
            q.code(boundary + 4, AccessWidth::Word, AccessKind::Sequential),
            word_s
        );
    }
    let mut q = queue(ENABLE);
    q.code(0x0dff_fffc, AccessWidth::Word, AccessKind::Sequential);
    q.advance(u32::MAX);
    assert_eq!(q.stream.unwrap().ready, 0); // Never prefetch into cartridge save space.
}

#[test]
fn relevant_waitcnt_changes_clear_the_queue_but_phi_and_sram_fields_do_not() {
    for changed in [0, ENABLE | 4, ENABLE | 0x10, ENABLE | 0x80, ENABLE | 0x400] {
        let mut q = queue(ENABLE);
        q.code(PC, AccessWidth::Word, AccessKind::Sequential);
        q.advance(10);
        q.configure(changed);
        assert!(q.stream.is_none());
    }
    let mut q = queue(ENABLE);
    q.code(PC, AccessWidth::Word, AccessKind::Sequential);
    q.advance(10);
    let stream = q.stream;
    q.configure(ENABLE | 0x1803);
    assert_eq!(q.stream, stream);
    assert_eq!(q.code(PC + 4, AccessWidth::Word, AccessKind::Sequential), 1);
}

#[test]
fn disabled_prefetch_retains_raw_costs_and_never_starts_background_work() {
    let mut q = queue(0);
    for _ in 0..3 {
        assert_eq!(q.code(PC, AccessWidth::Word, AccessKind::Sequential), 6);
        assert_eq!(
            q.code(PC + 4, AccessWidth::Word, AccessKind::NonSequential),
            8
        );
        q.advance(u32::MAX);
        assert!(q.stream.is_none());
    }
}

#[test]
fn batched_progress_matches_single_clock_progress_for_seeded_event_sequences() {
    let mut fast = queue(ENABLE);
    let mut slow = fast;
    let mut seed = 0x1234_5678_u32;
    let mut address = PC;
    for _ in 0..10_000 {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let idle = seed >> 24;
        fast.advance(idle);
        for _ in 0..idle {
            slow.advance(1);
        }
        assert_eq!(fast, slow);
        let width = if address & 3 == 0 && seed & 1 == 0 {
            AccessWidth::Word
        } else {
            AccessWidth::Halfword
        };
        assert_eq!(
            fast.code(address, width, AccessKind::Sequential),
            slow.code(address, width, AccessKind::Sequential)
        );
        address += width.bytes();
    }
}
