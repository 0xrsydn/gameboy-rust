use super::*;
use gba_core::dma::DMA_BASE;

/// Wide-integer reference for signed low-word multiplication and floor division.
/// This avoids both host signed overflow and the emitted instruction sequence.
fn low_signed(value: i64) -> i64 {
    let bits = value.rem_euclid(1i64 << 32);
    if bits >= 1i64 << 31 {
        bits - (1i64 << 32)
    } else {
        bits
    }
}

/// ArcTan's result, then its r1 (negated squared ratio) and r3 (polynomial sum).
fn atan_parts(tangent: i32) -> (i32, u32, u32) {
    let a = low_signed(-low_signed(i64::from(tangent).pow(2)).div_euclid(16384));
    let mut polynomial = 0xa9i64;
    for coefficient in [0x390, 0x91c, 0xfb6, 0x16aa, 0x2081, 0x3651, 0xa2f9] {
        polynomial = low_signed(low_signed(polynomial * a).div_euclid(16384) + coefficient);
    }
    let result = low_signed(i64::from(tangent) * polynomial).div_euclid(65536) as i32;
    (result, a as u32, polynomial as u32)
}

fn atan_reference(tangent: i32) -> i32 {
    atan_parts(tangent).0
}

/// Eight-sector signed-coordinate reference with 32-bit wrapping ratios, and the r1 of the
/// inner ArcTan (`None` on an axis, where r1 keeps Y).
fn atan2_parts(x: i32, y: i32) -> (u32, Option<u32>) {
    if y == 0 {
        return (if x < 0 { 0x8000 } else { 0 }, None);
    }
    if x == 0 {
        return (if y < 0 { 0xc000 } else { 0x4000 }, None);
    }
    let ratio = |n: i32, d: i32| atan_parts(n.wrapping_shl(14).wrapping_div(d));
    let (base, sign, (angle, a, _)) = if y > 0 {
        if x > 0 && x >= y {
            (0, 1, ratio(y, x))
        } else if x < 0 && x.wrapping_neg() >= y {
            (0x8000, 1, ratio(y, x))
        } else {
            (0x4000, -1, ratio(x, y))
        }
    } else if x < 0 && x.wrapping_neg() > y.wrapping_neg() {
        (0x8000, 1, ratio(y, x))
    } else if x > 0 && x >= y.wrapping_neg() {
        (0x10000, 1, ratio(y, x))
    } else {
        (0xc000, -1, ratio(x, y))
    };
    (((base + sign * angle) & 0xffff) as u32, Some(a))
}

fn atan2_reference(x: i32, y: i32) -> u32 {
    atan2_parts(x, y).0
}

/// ArcTan2 leaves this constant in r3 on hardware.
const ATAN2_R3: u32 = 0x170;

/// Registers after the call: r0, r1, and r3 are outputs; every other register is preserved.
fn expected_registers(service: u8, x: i32, y: i32, before: &[u32]) -> Vec<u32> {
    let mut registers = before[..15].to_vec();
    if service == 9 {
        let (result, a, b) = atan_parts(x);
        registers[0] = result as u32;
        registers[1] = a;
        registers[3] = b;
    } else {
        let (result, a) = atan2_parts(x, y);
        registers[0] = result;
        registers[1] = a.unwrap_or(y as u32);
        registers[3] = ATAN2_R3;
    }
    registers
}

fn run(service: u8, x: i32, y: i32, thumb: bool) -> u32 {
    let (mut m, pc) = call(service, thumb, [x as u32, y as u32, 0xcafe_babe]);
    let before = m.cpu().clone();
    let guard = bios::SVC_STACK - if service == 9 { 32 } else { 44 };
    m.memory_mut().write32(guard, 0xa55a_1234).unwrap();
    finish(&mut m, pc);
    assert_eq!(
        &m.cpu().registers()[..15],
        &expected_registers(service, x, y, before.registers())[..],
        "service={service}, x={x}, y={y}"
    );
    assert_eq!(m.cpu().cpsr(), before.cpsr());
    assert_eq!(m.memory().read32(guard).unwrap(), 0xa55a_1234);
    m.cpu().registers()[0]
}

fn check_direction(x: i32, y: i32, thumb: bool) {
    let actual = run(0x0a, x, y, thumb);
    if x == 0 && y == 0 {
        return;
    }
    let ideal = f64::from(y).atan2(f64::from(x)) * 32768.0 / std::f64::consts::PI;
    let error = (f64::from(actual) - ideal + 32768.0).rem_euclid(65536.0) - 32768.0;
    assert!(
        error.abs() < 3.0,
        "({x},{y}) actual={actual} ideal={ideal} error={error}"
    );
}

#[test]
fn arctan_known_fixed_point_values_include_signed_results() {
    for (input, expected) in [
        (-16384, -8192),
        (-8192, -4836),
        (-1, -1),
        (0, 0),
        (1, 0),
        (8192, 4836),
        (16384, 8192),
    ] {
        for thumb in [false, true] {
            assert_eq!(run(9, input, 0x12345678, thumb) as i32, expected);
        }
    }
}

#[test]
fn arctan_dense_unit_interval_grid_matches_polynomial_and_real_angle() {
    for input in (-16384..=16384).step_by(16) {
        let actual = run(9, input, 0, input & 16 != 0) as i32;
        let ideal = (f64::from(input) / 16384.0).atan() * 32768.0 / std::f64::consts::PI;
        assert!((f64::from(actual) - ideal).abs() < 2.0, "input={input}");
    }
}

#[test]
fn arctan_full_signed_domain_uses_wrapped_products_not_host_float() {
    for input in (-32768..=32767).step_by(257) {
        run(9, input, 0, input & 1 != 0);
    }
    // Known out-of-unit-range accuracy problems are retained, not corrected with host atan.
    assert_eq!(run(9, -32768, 0, false) as i32, -5795);
    assert_eq!(run(9, 32767, 0, true), 5848);
}

#[test]
fn arctan_seeded_inputs_match_wide_integer_reference() {
    let mut seed = 0x9a2b_14cdu32;
    for index in 0..1024 {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        run(9, i32::from((seed >> 16) as i16), 0, index % 2 == 0);
    }
}

#[test]
fn negative_arctan_rounds_down_instead_of_truncating_toward_zero() {
    for input in [1, 2, 3, 127, 8191, 8192, 16383, 16384] {
        let positive = run(9, input, 0, false) as i32;
        let negative = run(9, -input, 0, true) as i32;
        assert!([0, -1].contains(&(positive + negative)));
    }
    assert_eq!(run(9, -1, 0, true), u32::MAX);
}

#[test]
fn arctan2_axes_origin_and_diagonals_have_exact_turn_angles() {
    for thumb in [false, true] {
        for (x, y, expected) in [
            (0, 0, 0),
            (1, 0, 0),
            (-32768, 0, 0x8000),
            (0, 32767, 0x4000),
            (0, -32768, 0xc000),
            (1, 1, 0x2000),
            (-1, 1, 0x6000),
            (-1, -1, 0xa000),
            (1, -1, 0xe000),
            (-32768, -32768, 0xa000),
            (32767, 32767, 0x2000),
        ] {
            assert_eq!(run(0x0a, x, y, thumb), expected);
        }
    }
}

#[test]
fn arctan2_all_quadrants_and_signed_extremes_match_both_references() {
    let values = [
        -32768, -32767, -16384, -1024, -2, -1, 0, 1, 2, 1024, 16384, 32766, 32767,
    ];
    for x in values {
        for y in values {
            check_direction(x, y, (x ^ y) & 1 != 0);
        }
    }
}

#[test]
fn arctan2_sector_boundaries_handle_equal_and_adjacent_magnitudes() {
    for magnitude in [1, 2, 3, 127, 1024, 16384, 32766] {
        for delta in [-1, 0, 1] {
            for sx in [-1, 1] {
                for sy in [-1, 1] {
                    check_direction(sx * magnitude, sy * (magnitude + delta), delta != 0);
                }
            }
        }
    }
}

#[test]
fn arctan2_seeded_signed_coordinates_match_integer_and_float_references() {
    let mut seed = 0x5017_e3b9u32;
    for index in 0..1024 {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let x = i32::from((seed >> 16) as i16);
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let y = i32::from((seed >> 16) as i16);
        check_direction(x, y, index % 2 != 0);
    }
}

#[test]
fn arctan2_preserves_direction_when_coordinates_scale_equally() {
    for (x, y) in [
        (3, 7),
        (7, 3),
        (-3, 7),
        (-7, 3),
        (-3, -7),
        (-7, -3),
        (3, -7),
        (7, -3),
    ] {
        let expected = run(0x0a, x, y, false);
        for scale in [2, 7, 31, 4096] {
            assert_eq!(run(0x0a, x * scale, y * scale, true), expected);
        }
    }
}

#[test]
fn user_system_modes_masks_and_all_nonresult_registers_are_preserved() {
    for service in [9, 0x0a] {
        for status in [0x10, 0x50, 0x90, 0xd0, 0x1f, 0x5f, 0x9f, 0xdf] {
            for thumb in [false, true] {
                let (mut m, pc) = call_status(
                    service,
                    thumb,
                    [(-1234i32) as u32, 2345, 0xabcd_ef01],
                    status,
                );
                let before = m.cpu().clone();
                finish(&mut m, pc);
                assert_eq!(
                    &m.cpu().registers()[..15],
                    &expected_registers(service, -1234, 2345, before.registers())[..]
                );
                assert_eq!(m.cpu().cpsr(), before.cpsr());
            }
        }
    }
}

#[test]
fn every_caller_flag_combination_survives_angle_calculation() {
    for service in [9, 0x0au32] {
        for flags in 0..16u32 {
            let mut m = bios::boot(words(&[
                0xe3a0_0a02,         // MOV r0,#0x2000
                0xe3a0_1901,         // MOV r1,#0x4000
                0xe328_f200 | flags, // MSR CPSR_f,#flags<<28
                0xef00_0000 | service << 16,
                0xeaff_fffe,
            ]))
            .unwrap();
            reach(&mut m, ROM_START + 12, 100);
            assert_eq!(m.cpu().cpsr() >> 28, flags);
            let before = m.cpu().clone();
            finish(&mut m, ROM_START + 16);
            assert_eq!(m.cpu().cpsr(), before.cpsr());
            assert_eq!(
                &m.cpu().registers()[..15],
                &expected_registers(service as u8, 8192, 16384, before.registers())[..]
            );
        }
    }
}

// Hardware results from the mGBA test suite's BIOS math tests, including inputs outside
// the signed 16-bit range: (service, r0, r1) -> (r0, r1, r3).
#[test]
fn full_word_arguments_match_hardware_results_and_side_effects() {
    for (service, input, expected) in [
        (9, [0, 0], [0, 0, 0xa2f9]),
        (9, [0x3fff, 0], [0x1fff, 0xffff_c002, 0x8001]),
        (9, [0x7fff, 0], [0x16d8, 0xffff_0004, 0x2_2db6]),
        (9, [0xc000, 0], [0xffff_c360, 0x1_c000, 0x1_0480]),
        (9, [0xffff, 0], [0xffff_a2fe, 8, 0xa2ff]),
        (9, [0xffff_0001, 0], [0x5d01, 8, 0xa2ff]),
        (9, [0xffff_8000, 0], [0xffff_e95d, 0xffff_0000, 0x2_2d45]),
        (0x0a, [0, 0], [0, 0, ATAN2_R3]),
        (0x0a, [0xffff_0000, 0], [0x8000, 0, ATAN2_R3]),
        (0x0a, [0, 1], [0x4000, 1, ATAN2_R3]),
        (0x0a, [0xffff_ffff, 1], [0x6000, 0xffff_c000, ATAN2_R3]),
        (0x0a, [0xffff_c000, 1], [0x7fff, 0, ATAN2_R3]),
        (0x0a, [0xffff_0000, 0x4000], [0x7604, 0xffff_fc00, ATAN2_R3]),
        (0x0a, [0xffff, 0x1_0000], [0x2001, 0xffff_c002, ATAN2_R3]),
        (0x0a, [0xffff, 0xffff_0000], [0xe000, 0xffff_c002, ATAN2_R3]),
        (
            0x0a,
            [0xffff_0000, 0xffff_0000],
            [0xa000, 0xffff_c000, ATAN2_R3],
        ),
        (
            0x0a,
            [0xffff_0001, 0xffff_0000],
            [0xa001, 0xffff_c002, ATAN2_R3],
        ),
    ] {
        for thumb in [false, true] {
            let (mut m, pc) = call(service, thumb, [input[0], input[1], 0xcafe_babe]);
            finish(&mut m, pc);
            let registers = m.cpu().registers();
            assert_eq!(
                [registers[0], registers[1], registers[3]],
                expected,
                "service={service:#x} input={input:x?}"
            );
            assert_eq!(registers[2], 0xcafe_babe);
            // The reference agrees with every hardware vector.
            assert_eq!(
                expected_registers(service, input[0] as i32, input[1] as i32, registers),
                registers[..15]
            );
        }
    }
}

#[test]
fn seeded_full_word_arguments_match_wrapping_references() {
    let mut seed = 0x2468_ace1u32;
    for index in 0..512 {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let x = seed as i32;
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let y = seed as i32;
        run(9, x, 0, index % 2 == 0);
        run(0x0a, x, y, index % 2 != 0);
        // Scaled-down coordinates exercise every sector without overflowing the ratio.
        run(0x0a, x >> 12, y >> 12, index % 2 == 0);
    }
}

#[test]
fn repeated_calls_restore_supervisor_stack_and_do_not_retain_ratio_state() {
    for service in [9u32, 0x0a] {
        let mut m = bios::boot(words(&[
            0xe3a0_0a02, // MOV r0,#0x2000
            0xe3a0_1901, // MOV r1,#0x4000
            0xef00_0000 | service << 16,
            0xeaff_fffb, // B ROM_START
        ]))
        .unwrap();
        reach(&mut m, ROM_START, 100);
        for _ in 0..100 {
            reach(&mut m, ROM_START + 8, 10);
            m.step().unwrap();
            assert_eq!(m.cpu().registers()[13], bios::SVC_STACK);
            reach(&mut m, ROM_START + 12, 1000);
            assert_eq!(m.cpu().registers()[13], SYSTEM_STACK);
            assert_eq!(
                m.cpu().registers()[0],
                if service == 9 {
                    atan_reference(8192) as u32
                } else {
                    atan2_reference(8192, 16384)
                }
            );
            m.step().unwrap();
            assert_eq!(m.cpu().pc(), ROM_START);
        }
    }
}

#[test]
fn devices_and_dma_progress_and_pending_irq_waits_until_return() {
    for service in [9, 0x0a] {
        let (mut m, pc) = call(service, false, [8192, 16384, 0]);
        m.step().unwrap(); // Enter SWI before enabling IRQ sources.
        m.memory_mut().write16(IE, 8).unwrap();
        m.memory_mut().write16(IME, 1).unwrap();
        m.memory_mut().write32(TIMER_BASE, 0x00c0_fff0).unwrap();
        m.memory_mut().write32(SOURCE, 0x8765_4321).unwrap();
        m.memory_mut().write32(DMA_BASE, SOURCE).unwrap();
        m.memory_mut().write32(DMA_BASE + 4, SOURCE + 4).unwrap();
        m.memory_mut().write32(DMA_BASE + 8, 0x8400_0001).unwrap();
        let mut dma = 0;
        for _ in 0..1000 {
            if m.cpu().pc() == pc {
                break;
            }
            let step = m.step().unwrap();
            assert_ne!(step, StepKind::IrqEntry);
            if matches!(step, StepKind::Dma { channel: 0 }) {
                dma += 1;
            }
        }
        assert_eq!(m.cpu().pc(), pc);
        assert_eq!(dma, 1);
        assert_eq!(m.memory().read32(SOURCE + 4).unwrap(), 0x8765_4321);
        assert_eq!(m.memory().read16(IF).unwrap() & 8, 8);
        assert_eq!(m.memory().read16(IME).unwrap(), 1);
        assert_eq!(m.memory().read16(IRQ_FLAGS).unwrap(), 0);
        assert_eq!(
            m.cpu().registers()[0],
            if service == 9 {
                atan_reference(8192) as u32
            } else {
                atan2_reference(8192, 16384)
            }
        );
        assert_eq!(m.step().unwrap(), StepKind::IrqEntry);
    }
}
