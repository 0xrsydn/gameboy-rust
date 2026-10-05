use super::*;
use gba_core::{bios::INVALID_ARGUMENT_TRAP, dma::DMA_BASE};

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

fn atan_reference(tangent: i32) -> i32 {
    let a = -low_signed(i64::from(tangent).pow(2)).div_euclid(16384);
    let mut polynomial = 0xa9i64;
    for coefficient in [0x390, 0x91c, 0xfb6, 0x16aa, 0x2081, 0x3651, 0xa2f9] {
        polynomial = low_signed(low_signed(polynomial * a).div_euclid(16384) + coefficient);
    }
    low_signed(i64::from(tangent) * polynomial).div_euclid(65536) as i32
}

/// Eight-sector signed-coordinate reference; production uses absolute magnitudes.
fn atan2_reference(x: i32, y: i32) -> u32 {
    if y == 0 {
        return if x < 0 { 0x8000 } else { 0 };
    }
    if x == 0 {
        return if y < 0 { 0xc000 } else { 0x4000 };
    }
    let ratio = |n: i32, d: i32| atan_reference((i64::from(n) * 16384 / i64::from(d)) as i32);
    let angle = if y > 0 {
        if x > 0 && x >= y {
            ratio(y, x)
        } else if x < 0 && -x >= y {
            0x8000 + ratio(y, x)
        } else {
            0x4000 - ratio(x, y)
        }
    } else if x < 0 && -x > -y {
        0x8000 + ratio(y, x)
    } else if x > 0 && x >= -y {
        0x10000 + ratio(y, x)
    } else {
        0xc000 - ratio(x, y)
    };
    (angle & 0xffff) as u32
}

fn run(service: u8, x: i32, y: i32, thumb: bool) -> u32 {
    let (mut m, pc) = call(service, thumb, [x as u32, y as u32, 0xcafe_babe]);
    let before = m.cpu().clone();
    let guard = bios::SVC_STACK - if service == 9 { 32 } else { 44 };
    m.memory_mut().write32(guard, 0xa55a_1234).unwrap();
    finish(&mut m, pc);
    assert_eq!(&m.cpu().registers()[1..15], &before.registers()[1..15]);
    assert_eq!(m.cpu().cpsr(), before.cpsr());
    assert_eq!(m.memory().read32(guard).unwrap(), 0xa55a_1234);
    let actual = m.cpu().registers()[0];
    let expected = if service == 9 {
        atan_reference(x) as u32
    } else {
        atan2_reference(x, y)
    };
    assert_eq!(actual, expected, "service={service}, x={x}, y={y}");
    actual
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
                assert_eq!(&m.cpu().registers()[1..15], &before.registers()[1..15]);
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
            assert_eq!(&m.cpu().registers()[1..15], &before.registers()[1..15]);
            assert_eq!(
                m.cpu().registers()[0],
                if service == 9 {
                    atan_reference(8192) as u32
                } else {
                    atan2_reference(8192, 16384)
                }
            );
        }
    }
}

#[test]
fn noncanonical_signed_sixteen_bit_arguments_are_repeatable_diagnostics() {
    for value in [
        0x8000,
        0xffff,
        0x10000,
        0xffff_7fff,
        0x7fff_ffff,
        0x8000_0000,
    ] {
        for (service, args) in [
            (9, [value, 0, 0]),
            (0x0a, [value, 1, 0]),
            (0x0a, [1, value, 0]),
        ] {
            let (mut m, _) = call(service, false, args);
            let mut failed = false;
            for _ in 0..150 {
                let before = m.cpu().clone();
                let cycles = m.cycles();
                let timing = m.last_timing();
                if let Err(error) = m.step() {
                    assert!(matches!(
                        error,
                        MachineError::Cpu(CpuError::UnsupportedInstruction {
                            instruction: INVALID_ARGUMENT_TRAP,
                            ..
                        })
                    ));
                    assert_eq!(m.cpu(), &before);
                    assert_eq!(m.cycles(), cycles);
                    assert_eq!(m.last_timing(), timing);
                    assert_eq!(m.step(), Err(error));
                    failed = true;
                    break;
                }
            }
            assert!(failed);
        }
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
