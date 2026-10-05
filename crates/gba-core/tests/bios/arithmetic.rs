use super::*;
use gba_core::bios::INVALID_ARGUMENT_TRAP;

fn divide(number: i32, denominator: i32, thumb: bool, service: u8) {
    let inputs = if service == 6 {
        [number as u32, denominator as u32]
    } else {
        [denominator as u32, number as u32]
    };
    let (mut machine, return_pc) = call(service, thumb, [inputs[0], inputs[1], 0xcafe_babe]);
    let before = machine.cpu().clone();
    finish(&mut machine, return_pc);
    // Widening avoids host overflow for INT_MIN / -1.
    let quotient = i64::from(number) / i64::from(denominator);
    let remainder = i64::from(number) % i64::from(denominator);
    assert_eq!(
        machine.cpu().registers()[0],
        quotient as u32,
        "{number}/{denominator}"
    );
    assert_eq!(machine.cpu().registers()[1], remainder as u32);
    assert_eq!(machine.cpu().registers()[3], quotient.unsigned_abs() as u32);
    assert_eq!(machine.cpu().registers()[2], before.registers()[2]);
    assert_eq!(
        &machine.cpu().registers()[4..15],
        &before.registers()[4..15]
    );
    assert_eq!(machine.cpu().cpsr(), before.cpsr());
}

#[test]
fn division_handles_signs_zero_numerator_and_signed_boundaries() {
    for service in [6, 7] {
        for thumb in [false, true] {
            for number in [
                i32::MIN,
                i32::MIN + 1,
                -1234,
                -17,
                -1,
                0,
                1,
                17,
                1234,
                i32::MAX,
            ] {
                for denominator in [i32::MIN, -123, -10, -1, 1, 10, 123, i32::MAX] {
                    divide(number, denominator, thumb, service);
                }
            }
        }
    }
}

#[test]
fn division_matches_wide_integer_reference_for_seeded_inputs() {
    let mut state = 0x1a2b_3c4d_u32;
    for index in 0..1024 {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        let number = state as i32;
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        let denominator = (state | 1) as i32;
        divide(
            number,
            denominator,
            index & 1 != 0,
            if index & 2 == 0 { 6 } else { 7 },
        );
    }
}

#[test]
fn division_by_zero_is_a_repeatable_diagnostic_not_an_endless_loop() {
    for service in [6, 7] {
        for thumb in [false, true] {
            for number in [0, 1, u32::MAX, 0x8000_0000] {
                let operands = if service == 6 {
                    [number, 0, 0]
                } else {
                    [0, number, 0]
                };
                let (mut machine, _) = call(service, thumb, operands);
                let error = (0..100)
                    .find_map(|_| machine.step().err())
                    .expect("bounded division diagnostic");
                assert!(matches!(
                    error,
                    MachineError::Cpu(CpuError::UnsupportedInstruction {
                        instruction: INVALID_ARGUMENT_TRAP,
                        ..
                    })
                ));
                let before = machine.cpu().clone();
                let cycles = machine.cycles();
                let timing = machine.last_timing();
                assert_eq!(machine.step(), Err(error));
                assert_eq!(machine.cpu(), &before);
                assert_eq!(machine.cycles(), cycles);
                assert_eq!(machine.last_timing(), timing);
            }
        }
    }
}

fn square_root(value: u32, thumb: bool) {
    let (mut machine, return_pc) = call(8, thumb, [value, 0x1234_5678, 0xabcd_ef01]);
    let before = machine.cpu().clone();
    finish(&mut machine, return_pc);
    let root = u64::from(machine.cpu().registers()[0]);
    assert!(root * root <= u64::from(value));
    assert!((root + 1) * (root + 1) > u64::from(value));
    assert!(root <= 0xffff);
    assert_eq!(
        &machine.cpu().registers()[1..15],
        &before.registers()[1..15]
    );
    assert_eq!(machine.cpu().cpsr(), before.cpsr());
}

#[test]
fn sqrt_handles_perfect_squares_adjacent_values_and_u32_maximum() {
    for thumb in [false, true] {
        for root in [0_u32, 1, 2, 3, 255, 256, 32767, 32768, 65534, 65535] {
            let square = root * root;
            if square != 0 {
                square_root(square - 1, thumb);
            }
            square_root(square, thumb);
            square_root(square + 1, thumb);
        }
        square_root(u32::MAX, thumb);
    }
}

#[test]
fn sqrt_result_satisfies_integer_bounds_for_seeded_inputs() {
    let mut state = 0x9876_5432_u32;
    for index in 0..512 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        square_root(state, index & 1 != 0);
    }
}

#[test]
fn arithmetic_services_preserve_user_mode_and_interrupt_masks() {
    for service in [6, 7, 8] {
        for status in [0x10, 0x90, 0x9f, 0xdf] {
            for thumb in [false, true] {
                let (mut machine, return_pc) =
                    call_status(service, thumb, [144, 12, 0x55aa], status);
                let before = machine.cpu().clone();
                finish(&mut machine, return_pc);
                assert_eq!(machine.cpu().cpsr(), before.cpsr());
                assert_eq!(machine.cpu().registers()[2], before.registers()[2]);
                assert_eq!(
                    &machine.cpu().registers()[4..15],
                    &before.registers()[4..15]
                );
            }
        }
    }
}

#[test]
fn arithmetic_executes_real_instructions_and_keeps_device_clocks_running() {
    let (mut machine, return_pc) = call(6, false, [1234, 10, 0]);
    machine.memory_mut().write16(IE, 8).unwrap();
    machine.memory_mut().write16(IME, 1).unwrap();
    machine
        .memory_mut()
        .write32(TIMER_BASE, 0x00c0_fff0)
        .unwrap();
    let start = machine.cycles();
    let mut steps = 0;
    loop {
        assert_eq!(machine.step().unwrap(), StepKind::Instruction);
        steps += 1;
        if machine.cpu().pc() == return_pc {
            break;
        }
        assert!(steps < 1000);
    }
    assert!(steps >= 32 * 7);
    assert!(machine.cycles() > start + 16);
    assert_eq!(machine.memory().read16(IF).unwrap(), 8);
    assert_eq!(machine.cpu().registers()[0], 123);
    assert_eq!(machine.cpu().registers()[1], 4);
    assert_eq!(machine.step().unwrap(), StepKind::IrqEntry); // Delivery deferred until service return.
}
