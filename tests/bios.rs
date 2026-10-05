use gba_rust::{
    bios::{self, IRQ_FLAGS, IRQ_HANDLER, SYSTEM_STACK, UNSUPPORTED_TRAP},
    cpu::{CpuError, InstructionSet, Mode},
    display::VBLANK_START,
    io::{DISPSTAT, IE, IF, IME, POSTFLG, TIMER_BASE},
    machine::{Machine, MachineError, StepKind},
    memory::{Memory, MemoryError, BIOS_SIZE, ROM_START, VRAM_START},
};

const SOURCE: u32 = 0x0200_0000;
const DEST: u32 = 0x0201_0000;
const CALLBACK: u32 = 0x0300_0100;
const ROM_DATA: u32 = ROM_START + 13 * 4;

#[path = "bios/angles.rs"]
mod angles;
#[path = "bios/arithmetic.rs"]
mod arithmetic;
#[path = "bios/bit_unpack.rs"]
mod bit_unpack;
#[path = "bios/differential.rs"]
mod differential;
#[path = "bios/huffman.rs"]
mod huffman;
#[path = "bios/lz77.rs"]
mod lz77;
#[path = "bios/run_length.rs"]
mod run_length;

fn words(code: &[u32]) -> Vec<u8> {
    code.iter().flat_map(|word| word.to_le_bytes()).collect()
}

fn reach(machine: &mut Machine, pc: u32, max_steps: usize) {
    for _ in 0..max_steps {
        if machine.cpu().pc() == pc {
            return;
        }
        machine.step().unwrap();
    }
    panic!(
        "did not reach {pc:#x}; pc={:#x}, cpsr={:#x}",
        machine.cpu().pc(),
        machine.cpu().cpsr()
    );
}

/// Boot, load operands and status through CPU instructions, then stop just before SWI.
fn call(service: u8, thumb: bool, operands: [u32; 3]) -> (Machine, u32) {
    call_status(service, thumb, operands, 0x1f)
}

fn call_status(service: u8, thumb: bool, operands: [u32; 3], status: u32) -> (Machine, u32) {
    call_status_data(service, thumb, operands, status, &[])
}

fn call_status_data(
    service: u8,
    thumb: bool,
    operands: [u32; 3],
    status: u32,
    data: &[u8],
) -> (Machine, u32) {
    let mut code = [
        0xe59f_0020, // LDR r0,[pc,#32] (offset 40)
        0xe59f_1020,
        0xe59f_2020,
        0xe3a0_30a3,                                   // MOV r3,#0xa3
        0xe3a0_c05c,                                   // MOV r12,#0x5c
        0xe328_f480,                                   // MSR CPSR_f,#0x80000000
        if thumb { 0xe28f_e001 } else { 0xe1a0_e00f }, // ADD lr,pc,#1 / MOV lr,pc
        if thumb { 0xe12f_ff1e } else { 0xe1a0_0000 }, // BX lr / NOP
        if thumb {
            0xe7fe_0000 | 0xdf00 | u32::from(service)
        } else {
            0xef00_0000 | u32::from(service) << 16
        },
        0xeaff_fffe,
        operands[0],
        operands[1],
        operands[2],
    ];
    if status != 0x1f {
        code[3] = 0xe321_f000 | status;
    } // MSR CPSR_c,#status
    let mut rom = words(&code);
    assert_eq!(rom.len() as u32, ROM_DATA - ROM_START);
    rom.extend_from_slice(data);
    let mut machine = bios::boot(rom).unwrap();
    reach(&mut machine, ROM_START + 32, 100);
    assert_eq!(
        machine.cpu().instruction_set(),
        if thumb {
            InstructionSet::Thumb
        } else {
            InstructionSet::Arm
        }
    );
    (machine, ROM_START + if thumb { 34 } else { 36 })
}

fn finish(machine: &mut Machine, return_pc: u32) {
    machine.step().unwrap(); // Execute SWI, not a host service call.
    reach(machine, return_pc, 100_000);
}

/// Original ARM callback: acknowledge IF, record BIOS flags, then return to dispatcher.
fn callback(bus: &mut Memory) {
    let code = [
        0xe280_0c02, // ADD r0,r0,#0x200 (r0=I/O base from dispatcher)
        0xe1d0_10b2, // LDRH r1,[r0,#2]
        0xe1c0_10b2, // STRH r1,[r0,#2]
        0xe3a0_0403, // MOV r0,#0x03000000
        0xe280_0c7f, // ADD r0,r0,#0x7f00
        0xe1d0_2fb8, // LDRH r2,[r0,#0xf8]
        0xe182_2001, // ORR r2,r2,r1
        0xe1c0_2fb8, // STRH r2,[r0,#0xf8]
        0xe12f_ff1e, // BX lr
    ];
    for (index, instruction) in code.into_iter().enumerate() {
        bus.write32(CALLBACK + index as u32 * 4, instruction)
            .unwrap();
    }
    bus.write32(IRQ_HANDLER, CALLBACK).unwrap();
}

#[test]
fn original_image_is_deterministic_and_boot_enters_arm_system_mode() {
    assert_eq!(bios::image().len(), BIOS_SIZE);
    assert_eq!(bios::image(), bios::image());
    let mut machine = bios::boot(words(&[0xeaff_fffe])).unwrap();
    assert_eq!(machine.cpu().mode(), Mode::Supervisor);
    assert_eq!(machine.cpu().pc(), 0);
    reach(&mut machine, ROM_START, 100);
    assert_eq!(machine.cpu().mode(), Mode::System);
    assert_eq!(machine.cpu().instruction_set(), InstructionSet::Arm);
    assert_eq!(machine.cpu().cpsr(), 0x1f);
    assert_eq!(machine.cpu().registers()[13], SYSTEM_STACK);
    assert_eq!(machine.memory().read8(POSTFLG).unwrap(), 1);
    assert_eq!(machine.memory().read32(IRQ_FLAGS).unwrap(), 0);
    assert_eq!(machine.memory().read32(IRQ_HANDLER).unwrap(), 0);
    assert!(machine.cycles() > 0);
}

#[test]
fn cpu_set_copies_both_widths_from_arm_and_thumb_and_preserves_caller() {
    for thumb in [false, true] {
        for word in [false, true] {
            let (mut machine, return_pc) = call(
                0x0b,
                thumb,
                [SOURCE, DEST, 4 | if word { 1 << 26 } else { 0 }],
            );
            for index in 0..4 {
                machine
                    .memory_mut()
                    .write32(SOURCE + index * 4, 0x1234_5678 + index)
                    .unwrap();
            }
            let before = machine.cpu().clone();
            finish(&mut machine, return_pc);
            assert_eq!(&machine.cpu().registers()[..15], &before.registers()[..15]);
            assert_eq!(machine.cpu().cpsr(), before.cpsr());
            for offset in 0..if word { 16 } else { 8 } {
                assert_eq!(
                    machine.memory().read8(DEST + offset).unwrap(),
                    machine.memory().read8(SOURCE + offset).unwrap()
                );
            }
        }
    }
}

#[test]
fn intr_wait_consumes_preexisting_selected_flags_without_sleep() {
    for thumb in [false, true] {
        let (mut machine, return_pc) = call(4, thumb, [0, 0x10, 0x1234]);
        machine.memory_mut().write16(IRQ_FLAGS, 0x19).unwrap();
        let before = machine.cpu().clone();
        machine.step().unwrap();
        for _ in 0..100 {
            if machine.cpu().pc() == return_pc {
                break;
            }
            assert_ne!(machine.step().unwrap(), StepKind::HaltIdle);
        }
        assert_eq!(machine.cpu().pc(), return_pc);
        assert_eq!(machine.memory().read16(IRQ_FLAGS).unwrap(), 9);
        assert_eq!(machine.memory().read16(IME).unwrap(), 1);
        assert_eq!(machine.cpu().cpsr(), before.cpsr());
        assert_eq!(&machine.cpu().registers()[..15], &before.registers()[..15]);
    }
}

#[test]
fn vblank_wait_halts_dispatches_irq_and_returns_through_original_code() {
    let (mut machine, return_pc) = call(5, false, [0x123, 0x456, 0x789]);
    callback(machine.memory_mut());
    machine.memory_mut().write16(DISPSTAT, 8).unwrap();
    machine.memory_mut().write16(IE, 1).unwrap();
    let before = machine.cpu().clone();
    machine.step().unwrap();
    let mut slept = false;
    let mut irq = 0;
    for _ in 0..1000 {
        if machine.cpu().pc() == return_pc {
            break;
        }
        match machine.step().unwrap() {
            StepKind::HaltIdle => slept = true,
            StepKind::IrqEntry => irq += 1,
            _ => {}
        }
    }
    assert!(slept);
    assert_eq!(irq, 1);
    assert_eq!(machine.cpu().pc(), return_pc);
    assert_eq!(machine.cpu().cpsr(), before.cpsr());
    assert_eq!(&machine.cpu().registers()[..15], &before.registers()[..15]);
    assert_eq!(machine.memory().read16(IF).unwrap(), 0);
    assert_eq!(machine.memory().read16(IRQ_FLAGS).unwrap(), 0);
    assert_eq!(machine.memory().read16(IME).unwrap(), 1);
    assert!(machine.cycles() > u64::from(VBLANK_START));
}

#[test]
fn unsupported_services_fail_at_an_explicit_instruction_trap() {
    for service in [0, 1, 3, 0x0d, 0x0e, 0x0f, 0x19, 0xff] {
        let (mut machine, _) = call(service, false, [0, 0, 0]);
        let error = (0..100)
            .find_map(|_| machine.step().err())
            .expect("unsupported SWI must fail");
        assert!(matches!(
            error,
            MachineError::Cpu(CpuError::UnsupportedInstruction {
                instruction: UNSUPPORTED_TRAP,
                ..
            })
        ));
    }
}

#[test]
fn cpu_set_fill_reads_one_value_and_writes_exact_count() {
    for thumb in [false, true] {
        for word in [false, true] {
            let control = 3 | (1 << 24) | if word { 1 << 26 } else { 0 };
            let (mut machine, return_pc) = call(0x0b, thumb, [SOURCE, DEST, control]);
            machine.memory_mut().write32(SOURCE, 0xaabb_ccdd).unwrap();
            for offset in 0..5 {
                machine
                    .memory_mut()
                    .write32(DEST + offset * 4, 0x9999_9999)
                    .unwrap();
            }
            finish(&mut machine, return_pc);
            let bytes = if word { 12 } else { 6 };
            for offset in 0..bytes {
                assert_eq!(
                    machine.memory().read8(DEST + offset).unwrap(),
                    [0xdd, 0xcc, 0xbb, 0xaa][(offset % if word { 4 } else { 2 }) as usize]
                );
            }
            assert_eq!(machine.memory().read16(DEST + bytes).unwrap(), 0x9999);
        }
    }
}

#[test]
fn cpu_fast_set_rounds_up_to_eight_words_for_copy_and_fill() {
    for thumb in [false, true] {
        for fill in [false, true] {
            for count in [1_u32, 7, 8, 9, 15, 16, 17] {
                let (mut machine, return_pc) = call(
                    0x0c,
                    thumb,
                    [SOURCE, DEST, count | if fill { 1 << 24 } else { 0 }],
                );
                let rounded = (count + 7) & !7;
                for index in 0..rounded + 1 {
                    machine
                        .memory_mut()
                        .write32(SOURCE + index * 4, 0x1000 + index)
                        .unwrap();
                    machine
                        .memory_mut()
                        .write32(DEST + index * 4, 0xcccc_cccc)
                        .unwrap();
                }
                let before = machine.cpu().clone();
                finish(&mut machine, return_pc);
                for index in 0..rounded {
                    assert_eq!(
                        machine.memory().read32(DEST + index * 4).unwrap(),
                        0x1000 + if fill { 0 } else { index }
                    );
                }
                assert_eq!(
                    machine.memory().read32(DEST + rounded * 4).unwrap(),
                    0xcccc_cccc
                );
                assert_eq!(&machine.cpu().registers()[..15], &before.registers()[..15]);
                assert_eq!(machine.cpu().cpsr(), before.cpsr());
            }
        }
    }
}

#[test]
fn fast_copy_reads_each_eight_word_block_before_writing_it() {
    let (mut machine, return_pc) = call(0x0c, false, [SOURCE, SOURCE + 4, 16]);
    let mut expected: Vec<u32> = (0..17).map(|index| 0x1234 + index).collect();
    for (index, value) in expected.iter().enumerate() {
        machine
            .memory_mut()
            .write32(SOURCE + index as u32 * 4, *value)
            .unwrap();
    }
    for start in [0, 8] {
        let block = expected[start..start + 8].to_vec();
        expected[start + 1..start + 9].copy_from_slice(&block);
    }
    finish(&mut machine, return_pc);
    for (index, value) in expected.into_iter().enumerate() {
        assert_eq!(
            machine.memory().read32(SOURCE + index as u32 * 4).unwrap(),
            value
        );
    }
}

#[test]
fn swi_callers_outside_user_and_system_modes_are_rejected() {
    for mode in [0x11, 0x12, 0x13, 0x17, 0x1b] {
        let (mut machine, _) = call_status(0x0b, false, [SOURCE, DEST, 0], mode);
        let error = (0..100).find_map(|_| machine.step().err()).unwrap();
        assert!(matches!(
            error,
            MachineError::Cpu(CpuError::UnsupportedInstruction {
                instruction: UNSUPPORTED_TRAP,
                ..
            })
        ));
    }
}

#[test]
fn zero_length_copies_do_not_read_source_or_write_destination() {
    for service in [0x0b, 0x0c] {
        for control in [0, 1 << 24, 1 << 26, (1 << 24) | (1 << 26), 0xfae0_0000] {
            let (mut machine, return_pc) =
                call(service, false, [0x0e00_0000, 0x0e00_0000, control]);
            finish(&mut machine, return_pc); // Both addresses would fail if accessed.
        }
    }
}

#[test]
fn copy_count_ignores_reserved_bits_and_fast_set_always_uses_words() {
    for service in [0x0b, 0x0c] {
        let (mut machine, return_pc) = call(service, false, [SOURCE, DEST, 1 | 0xfae0_0000]);
        machine.memory_mut().write32(SOURCE, 0x1234_abcd).unwrap();
        finish(&mut machine, return_pc);
        assert_eq!(
            machine.memory().read32(DEST).unwrap(),
            if service == 0x0b { 0xabcd } else { 0x1234_abcd }
        );
    }
}

#[test]
fn copying_to_video_ram_uses_halfword_and_word_bus_writes() {
    for service in [0x0b, 0x0c] {
        let (mut machine, return_pc) = call(service, false, [SOURCE, VRAM_START, 3]);
        machine.memory_mut().write32(SOURCE, 0x03e0_001f).unwrap();
        machine
            .memory_mut()
            .write32(SOURCE + 4, 0x7fff_7c00)
            .unwrap();
        finish(&mut machine, return_pc);
        assert_eq!(machine.memory().read32(VRAM_START).unwrap(), 0x03e0_001f);
        assert_eq!(machine.memory().read16(VRAM_START + 4).unwrap(), 0x7c00);
        assert_eq!(
            machine.memory().read16(VRAM_START + 6).unwrap(),
            if service == 0x0b { 0 } else { 0x7fff }
        );
    }
}

#[test]
fn copy_addresses_align_down_and_overlaps_use_instruction_order() {
    for word in [false, true] {
        let (mut machine, return_pc) = call(
            0x0b,
            false,
            [SOURCE + 3, DEST + 3, 1 | if word { 1 << 26 } else { 0 }],
        );
        machine.memory_mut().write32(SOURCE, 0x1234_abcd).unwrap();
        finish(&mut machine, return_pc);
        assert_eq!(
            machine.memory().read32(DEST).unwrap(),
            if word { 0x1234_abcd } else { 0x1234_0000 }
        );
    }
    let (mut machine, return_pc) = call(0x0b, false, [SOURCE, SOURCE + 2, 4]);
    machine.memory_mut().write32(SOURCE, 0xabcd_1234).unwrap();
    finish(&mut machine, return_pc);
    for offset in [0, 2, 4, 6, 8] {
        assert_eq!(machine.memory().read16(SOURCE + offset).unwrap(), 0x1234);
    }
}

#[test]
fn protected_or_wrapping_source_ranges_return_without_copying() {
    for service in [0x0b, 0x0c] {
        for source in [0, 0x3ffc, 0x0100_0000, 0x01ff_fffc, 0xffff_fffc] {
            let (mut machine, return_pc) = call(service, false, [source, DEST, 2 | (1 << 26)]);
            machine.memory_mut().write32(DEST, 0x1234_5678).unwrap();
            finish(&mut machine, return_pc);
            assert_eq!(machine.memory().read32(DEST).unwrap(), 0x1234_5678);
        }
    }
}

#[test]
fn copy_failure_keeps_earlier_writes_but_not_partial_instruction_changes() {
    let (mut machine, _) = call(0x0b, false, [SOURCE, 0x07ff_fffc, 2 | (1 << 26)]);
    machine.memory_mut().write32(SOURCE, 0xaabb_ccdd).unwrap();
    let mut failed = false;
    for _ in 0..100 {
        let before = machine.cpu().clone();
        let timing = machine.last_timing();
        let cycles = machine.cycles();
        if let Err(error) = machine.step() {
            assert_eq!(
                error,
                MachineError::Cpu(MemoryError::ReadOnly(0x0800_0000).into())
            );
            assert_eq!(machine.cpu(), &before);
            assert_eq!(machine.cycles(), cycles);
            assert_eq!(machine.last_timing(), timing);
            assert_eq!(machine.memory().read32(0x07ff_fffc).unwrap(), 0xaabb_ccdd);
            failed = true;
            break;
        }
    }
    assert!(failed);
}

#[test]
fn halt_preserves_registers_and_ime_for_arm_and_thumb_callers() {
    for thumb in [false, true] {
        let (mut machine, return_pc) = call(2, thumb, [0x1234, 0x5678, 0x9abc]);
        machine.memory_mut().write16(IE, 8).unwrap();
        machine
            .memory_mut()
            .write32(TIMER_BASE, 0x00c0_fc18)
            .unwrap(); // IRQ after 1,000 cycles.
        let before = machine.cpu().clone();
        machine.step().unwrap();
        let mut slept = false;
        for _ in 0..200 {
            if machine.cpu().pc() == return_pc {
                break;
            }
            slept |= machine.step().unwrap() == StepKind::HaltIdle;
        }
        assert!(slept);
        assert_eq!(machine.cpu().pc(), return_pc);
        assert_eq!(machine.memory().read16(IME).unwrap(), 0);
        assert_eq!(machine.memory().read16(IF).unwrap(), 8);
        assert_eq!(&machine.cpu().registers()[..15], &before.registers()[..15]);
        assert_eq!(machine.cpu().cpsr(), before.cpsr());
    }
}

#[test]
fn halt_with_pending_irq_returns_without_an_idle_interval() {
    let (mut machine, return_pc) = call(2, false, [1, 2, 3]);
    request_timer(machine.memory_mut());
    machine.memory_mut().write16(IE, 8).unwrap();
    machine.step().unwrap();
    for _ in 0..100 {
        if machine.cpu().pc() == return_pc {
            break;
        }
        assert_ne!(machine.step().unwrap(), StepKind::HaltIdle);
    }
    assert_eq!(machine.cpu().pc(), return_pc);
}

fn request_timer(bus: &mut Memory) {
    bus.write32(TIMER_BASE, 0x00c0_ffff).unwrap();
    bus.advance_cycles(1);
    bus.write16(TIMER_BASE + 2, 0).unwrap();
}

#[test]
fn interrupt_wait_has_no_lost_wake_between_flag_check_and_halt() {
    for injection_step in 0..100 {
        let (mut machine, return_pc) = call(4, false, [0, 8, 0]);
        callback(machine.memory_mut());
        machine.memory_mut().write16(IE, 8).unwrap();
        for step in 0..500 {
            if step == injection_step {
                request_timer(machine.memory_mut());
            }
            if machine.cpu().pc() == return_pc {
                break;
            }
            machine.step().unwrap();
        }
        assert_eq!(
            machine.cpu().pc(),
            return_pc,
            "injection step {injection_step}"
        );
        assert_eq!(machine.memory().read16(IF).unwrap(), 0);
        assert_eq!(machine.memory().read16(IRQ_FLAGS).unwrap(), 0);
    }
}

#[test]
fn discard_old_flags_waits_for_a_new_callback_and_retains_other_flags() {
    for thumb in [false, true] {
        let (mut machine, return_pc) = call(4, thumb, [1, 8, 0]);
        callback(machine.memory_mut());
        machine.memory_mut().write16(IE, 8).unwrap();
        machine.memory_mut().write16(IRQ_FLAGS, 9).unwrap();
        for _ in 0..100 {
            if machine.halted() {
                break;
            }
            machine.step().unwrap();
        }
        assert!(machine.halted());
        assert_eq!(machine.memory().read16(IRQ_FLAGS).unwrap(), 1);
        request_timer(machine.memory_mut());
        reach(&mut machine, return_pc, 200);
        assert_eq!(machine.memory().read16(IRQ_FLAGS).unwrap(), 1);
    }
}

#[test]
fn unrelated_irq_does_not_complete_a_wait_for_vblank() {
    let (mut machine, return_pc) = call(4, false, [1, 1, 0]);
    callback(machine.memory_mut());
    machine.memory_mut().write16(IE, 9).unwrap();
    machine.memory_mut().write16(DISPSTAT, 8).unwrap();
    machine
        .memory_mut()
        .write32(TIMER_BASE, 0x00c0_fc18)
        .unwrap();
    let mut irqs = 0;
    for _ in 0..1000 {
        if machine.cpu().pc() == return_pc {
            break;
        }
        if machine.step().unwrap() == StepKind::IrqEntry {
            irqs += 1;
            machine.memory_mut().write16(TIMER_BASE + 2, 0).unwrap();
        }
    }
    assert_eq!(machine.cpu().pc(), return_pc);
    assert_eq!(irqs, 2);
    assert_eq!(machine.memory().read16(IRQ_FLAGS).unwrap(), 8);
    assert!(machine.cycles() > u64::from(VBLANK_START));
}

#[test]
fn wait_services_restore_user_system_and_irq_masked_caller_status() {
    for thumb in [false, true] {
        for status in [0x10, 0x90, 0x9f, 0xdf] {
            let (mut machine, return_pc) = call_status(4, thumb, [0, 8, 0], status);
            callback(machine.memory_mut());
            machine.memory_mut().write16(IE, 8).unwrap();
            let before = machine.cpu().clone();
            for _ in 0..100 {
                if machine.halted() {
                    break;
                }
                machine.step().unwrap();
            }
            assert!(machine.halted());
            request_timer(machine.memory_mut());
            reach(&mut machine, return_pc, 200);
            assert_eq!(machine.cpu().cpsr(), before.cpsr());
            assert_eq!(&machine.cpu().registers()[..15], &before.registers()[..15]);
            assert_eq!(machine.memory().read16(IME).unwrap(), 1);
        }
    }
}

#[test]
fn missing_or_misaligned_irq_callback_is_an_explicit_trap() {
    for pointer in [0, CALLBACK + 1, CALLBACK + 2] {
        let (mut machine, _) = call(5, false, [0, 0, 0]);
        machine.memory_mut().write32(IRQ_HANDLER, pointer).unwrap();
        machine.memory_mut().write16(IE, 8).unwrap();
        machine.memory_mut().write16(IME, 1).unwrap();
        request_timer(machine.memory_mut());
        let error = (0..100).find_map(|_| machine.step().err()).unwrap();
        assert!(matches!(
            error,
            MachineError::Cpu(CpuError::UnsupportedInstruction {
                instruction: UNSUPPORTED_TRAP,
                ..
            })
        ));
    }
}

#[test]
fn repeated_calls_restore_the_supervisor_stack() {
    let mut machine = bios::boot(words(&[0xe3a0_2000, 0xef0b_0000, 0xeaff_fffd])).unwrap();
    reach(&mut machine, ROM_START + 4, 100);
    for _ in 0..100 {
        machine.step().unwrap(); // SWI
        assert_eq!(machine.cpu().registers()[13], bios::SVC_STACK);
        reach(&mut machine, ROM_START + 8, 100);
        assert_eq!(machine.cpu().registers()[13], SYSTEM_STACK);
        machine.step().unwrap(); // Branch back to SWI.
        assert_eq!(machine.cpu().pc(), ROM_START + 4);
    }
}

#[test]
fn arm_swi_uses_upper_comment_byte_not_low_byte() {
    let mut machine = bios::boot(words(&[0xe3a0_2000, 0xef0b_ff03, 0xeaff_fffe])).unwrap();
    reach(&mut machine, ROM_START + 4, 100);
    finish(&mut machine, ROM_START + 8); // CpuSet(count=0), not Stop(3).
}

#[test]
fn callback_must_record_bios_flags_not_only_acknowledge_hardware_if() {
    let (mut machine, return_pc) = call(4, false, [0, 8, 0]);
    callback(machine.memory_mut());
    machine
        .memory_mut()
        .write32(CALLBACK + 12, 0xe12f_ff1e)
        .unwrap(); // Return after IF acknowledgement, omit flags.
    machine.memory_mut().write16(IE, 8).unwrap();
    request_timer(machine.memory_mut());
    for _ in 0..300 {
        machine.step().unwrap();
    }
    assert_ne!(machine.cpu().pc(), return_pc);
    assert!(machine.halted());
    assert_eq!(machine.memory().read16(IF).unwrap(), 0);
    assert_eq!(machine.memory().read16(IRQ_FLAGS).unwrap(), 0);
}

#[test]
fn supplying_an_image_is_optional_and_does_not_change_memory_new() {
    assert_eq!(
        Memory::new(vec![]).unwrap().read32(0),
        Err(MemoryError::Unmapped(0))
    );
    let mut bus = Memory::with_bios(vec![], bios::image()).unwrap();
    assert_eq!(bus.write32(0, 1), Err(MemoryError::ReadOnly(0)));
    assert_eq!(
        bus.read8(BIOS_SIZE as u32),
        Err(MemoryError::Unmapped(BIOS_SIZE as u32))
    );
}
