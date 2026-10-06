use gba_core::{
    bios,
    cpu::{Cpu, CpuError, InstructionSet, Mode},
    dma::DMA_BASE,
    input::Buttons,
    io::{HALTCNT, IE, IF, IME, KEYCNT, KEYINPUT, TIMER_BASE},
    machine::{Machine, MachineError, StepKind},
    memory::{Memory, MemoryError, BIOS_SIZE, ROM_START},
};

const IRQ: u16 = 1 << 12;

fn words(code: &[u32]) -> Vec<u8> {
    code.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn bus() -> Memory {
    Memory::new(vec![]).unwrap()
}

fn press(m: &mut Memory, buttons: u16) {
    m.set_buttons(Buttons::from_bits(buttons));
}

fn pending(m: &Memory) -> bool {
    m.read16(IF).unwrap() & IRQ != 0
}

fn ack(m: &mut Memory) {
    m.write16(IF, IRQ).unwrap();
    assert!(!pending(m));
}

#[test]
fn register_reset_masks_byte_merges_and_combined_word_access() {
    let mut m = bus();
    assert_eq!(m.read32(KEYINPUT).unwrap(), 0x03ff);
    for value in 0..=u16::MAX {
        m.write16(KEYCNT, value).unwrap();
        assert_eq!(m.read16(KEYCNT).unwrap(), value & 0xc3ff);
    }
    m.write16(KEYCNT, 0).unwrap();
    m.write8(KEYCNT + 1, 0xff).unwrap();
    assert_eq!(m.read16(KEYCNT).unwrap(), 0xc300);
    m.write8(KEYCNT, 0xa5).unwrap();
    assert_eq!(m.read16(KEYCNT).unwrap(), 0xc3a5);
    assert_eq!(m.read8(KEYCNT).unwrap(), 0xa5);
    assert_eq!(m.read8(KEYCNT + 1).unwrap(), 0xc3);
    press(&mut m, 0x201);
    m.write32(KEYINPUT, 0x0123_ffff).unwrap();
    assert_eq!(m.read32(KEYINPUT).unwrap(), 0x0123_01fe);
    m.write16(KEYINPUT, 0).unwrap();
    assert_eq!(m.read32(KEYINPUT).unwrap(), 0x0123_01fe);
    assert_eq!(m.cycles(), 0);
    assert_eq!(m.read32(KEYCNT), Err(MemoryError::Unaligned(KEYCNT)));
    for address in [KEYCNT + 4, KEYCNT + 0x400] {
        assert_eq!(m.read8(address), Err(MemoryError::Unmapped(address)));
        assert_eq!(m.write8(address, 0), Err(MemoryError::Unmapped(address)));
    }
}

#[test]
fn every_button_can_request_irq_independently_of_ie_and_ime() {
    for bit in 0..10 {
        for mode in [0, 0x8000] {
            let mut m = bus();
            m.write16(KEYCNT, 0x4000 | mode | (1 << bit)).unwrap();
            for other in 0..10 {
                press(&mut m, if other == bit { 0 } else { 1 << other });
                assert!(!pending(&m));
            }
            press(&mut m, 1 << bit);
            assert!(pending(&m));
            assert!(!m.irq_pending());
            m.write16(IE, IRQ).unwrap();
            assert!(!m.irq_pending());
            m.write16(IME, 1).unwrap();
            assert!(m.irq_pending());
        }
    }
}

#[test]
fn or_mode_requests_on_each_matching_input_sample_but_not_on_clock_updates() {
    let mut m = bus();
    m.write16(KEYCNT, 0x4003).unwrap();
    for buttons in [1, 1, 3, 2, 2] {
        press(&mut m, buttons);
        assert!(pending(&m));
        ack(&mut m);
        m.advance_cycles(12345);
        assert!(!pending(&m));
        assert_eq!(m.read16(KEYCNT).unwrap(), 0x4003);
    }
    press(&mut m, 4);
    assert!(!pending(&m));
    press(&mut m, 0);
    assert!(!pending(&m));
}

#[test]
fn and_mode_requires_selected_keys_but_allows_extra_keys_and_suppresses_identical_samples() {
    let mut m = bus();
    m.write16(KEYCNT, 0xc003).unwrap();
    for buttons in [0, 1, 2, 4] {
        press(&mut m, buttons);
        assert!(!pending(&m));
    }
    for buttons in [3, 7, 3] {
        press(&mut m, buttons);
        assert!(pending(&m)); // Includes changes to unselected keys in the polling model.
        ack(&mut m);
        press(&mut m, buttons);
        assert!(!pending(&m));
        m.write16(KEYCNT, 0xc003).unwrap();
        assert!(!pending(&m));
    }
    press(&mut m, 1);
    assert!(!pending(&m));
    press(&mut m, 3);
    assert!(pending(&m));
}

#[test]
fn empty_masks_follow_boolean_or_and_rules() {
    for buttons in [0, 1, 0x3ff] {
        let mut m = bus();
        press(&mut m, buttons);
        m.write16(KEYCNT, 0x4000).unwrap();
        assert!(!pending(&m));
        m.write16(KEYCNT, 0xc000).unwrap();
        assert!(pending(&m));
        ack(&mut m);
        press(&mut m, buttons);
        assert!(!pending(&m));
    }
}

#[test]
fn held_keys_are_sampled_on_control_writes_and_new_selection_rearms_and() {
    let mut m = bus();
    press(&mut m, 3);
    m.write16(KEYCNT, 0x0001).unwrap();
    assert!(!pending(&m));
    m.write16(KEYCNT, 0xc001).unwrap();
    assert!(pending(&m));
    ack(&mut m);
    m.write16(KEYCNT, 0xc003).unwrap();
    assert!(pending(&m)); // B was newly selected while already held.
    ack(&mut m);
    m.write16(KEYCNT, 0xc003).unwrap();
    assert!(!pending(&m));
    m.write16(KEYCNT, 0xc007).unwrap();
    assert!(!pending(&m)); // Newly selected, but not all selected keys held.
    m.write16(KEYCNT, 0xc003).unwrap();
    assert!(pending(&m));
}

#[test]
fn disabling_or_releasing_keys_does_not_acknowledge_if_and_disabled_sampling_keeps_history() {
    let mut m = bus();
    press(&mut m, 1);
    m.write16(KEYCNT, 0xc001).unwrap();
    assert!(pending(&m));
    m.write16(KEYCNT, 0x8001).unwrap();
    press(&mut m, 0);
    assert!(pending(&m));
    ack(&mut m);
    press(&mut m, 1);
    m.write16(KEYCNT, 0xc001).unwrap();
    assert!(!pending(&m)); // No matching-state history update while disabled.
    press(&mut m, 0);
    press(&mut m, 1);
    assert!(pending(&m));
}

#[test]
fn halfword_and_word_control_writes_never_sample_intermediate_byte_values() {
    for word in [false, true] {
        for (old, new, buttons) in [(0x4002, 0x0001, 1), (0xc001, 0x4000, 0)] {
            let mut m = bus();
            press(&mut m, buttons);
            m.write16(KEYCNT, old).unwrap();
            assert!(!pending(&m));
            if word {
                m.write32(KEYINPUT, u32::from(new) << 16).unwrap();
            } else {
                m.write16(KEYCNT, new).unwrap();
            }
            assert!(!pending(&m));
            assert_eq!(m.read16(KEYCNT).unwrap(), new);
        }
    }
    // Separate byte stores really do expose the intermediate enabled mask.
    let mut m = bus();
    press(&mut m, 1);
    m.write16(KEYCNT, 0x4002).unwrap();
    m.write8(KEYCNT, 1).unwrap();
    assert!(pending(&m));
    m.write8(KEYCNT + 1, 0).unwrap();
    assert!(pending(&m));
}

#[test]
fn keyinput_writes_and_reads_do_not_resample_an_acknowledged_request() {
    let mut m = bus();
    m.write16(KEYCNT, 0x4001).unwrap();
    press(&mut m, 1);
    ack(&mut m);
    m.write8(KEYINPUT, 0).unwrap();
    m.write8(KEYINPUT + 1, 0).unwrap();
    m.write16(KEYINPUT, 0).unwrap();
    assert_eq!(m.read16(KEYINPUT).unwrap(), 0x3fe);
    assert_eq!(m.read32(KEYINPUT).unwrap(), 0x4001_03fe);
    assert!(!pending(&m));
    // An actual KEYCNT write is a new sample, even if its value is unchanged.
    m.write16(KEYCNT, 0x4001).unwrap();
    assert!(pending(&m));
}

fn machine(masked: bool) -> Machine {
    let mut m = Machine::new(
        Cpu::new(ROM_START),
        Memory::new(words(&[
            if masked { 0xe321_f09f } else { 0xe321_f01f },
            0xe280_0001,
            0xeaff_fffe,
        ]))
        .unwrap(),
    );
    m.step().unwrap();
    m
}

#[test]
fn keypad_wakes_halt_only_through_ie_and_ignores_ime_and_cpu_masks_for_wake() {
    for masked in [false, true] {
        for ie in [0, IRQ] {
            for ime in [0, 1] {
                let mut m = machine(masked);
                m.memory_mut().write16(KEYCNT, 0x4001).unwrap();
                m.memory_mut().write16(IE, ie).unwrap();
                m.memory_mut().write16(IME, ime).unwrap();
                m.memory_mut().write8(HALTCNT, 0).unwrap();
                let before = m.cpu().clone();
                let cycles = m.cycles();
                let timing = m.last_timing();
                press(m.memory_mut(), 1);
                assert!(pending(m.memory()));
                assert_eq!(m.cpu(), &before);
                assert_eq!(m.cycles(), cycles);
                assert_eq!(m.last_timing(), timing);
                assert_eq!(m.halted(), ie == 0);
                let expected = if ie == 0 {
                    StepKind::HaltIdle
                } else if ime != 0 && !masked {
                    StepKind::IrqEntry
                } else {
                    StepKind::Instruction
                };
                assert_eq!(m.step().unwrap(), expected);
            }
        }
    }
}

#[test]
fn pending_keypad_request_wakes_on_ie_enable_and_ack_does_not_restore_halt() {
    let mut m = machine(false);
    m.memory_mut().write16(KEYCNT, 0x4001).unwrap();
    m.memory_mut().write8(HALTCNT, 0).unwrap();
    press(m.memory_mut(), 1);
    assert!(m.halted());
    m.memory_mut().write16(IE, IRQ).unwrap();
    assert!(!m.halted());
    ack(m.memory_mut());
    assert!(!m.halted());
    assert_eq!(m.step().unwrap(), StepKind::Instruction);
}

#[test]
fn control_write_can_wake_halt_with_an_already_held_key() {
    let mut m = machine(false);
    press(m.memory_mut(), 1);
    m.memory_mut().write16(IE, IRQ).unwrap();
    m.memory_mut().write8(HALTCNT, 0).unwrap();
    m.memory_mut().write16(KEYCNT, 0x4001).unwrap();
    assert!(!m.halted());
    assert_eq!(m.step().unwrap(), StepKind::Instruction); // IME remains zero.
}

#[test]
fn keypad_irq_entry_and_return_restore_arm_and_thumb_instruction_state() {
    for thumb in [false, true] {
        let rom = if thumb {
            let mut code = words(&[0xe28f_0001, 0xe12f_ff10]);
            code.extend(0x2107_u16.to_le_bytes());
            code
        } else {
            words(&[0xe3a0_1007])
        };
        let bios = 0xe25e_f004_u32.to_le_bytes().repeat(BIOS_SIZE / 4);
        let mut mem = Memory::with_bios(rom, bios).unwrap();
        let mut cpu = Cpu::new(ROM_START);
        if thumb {
            cpu.step(&mut mem).unwrap();
            cpu.step(&mut mem).unwrap();
        }
        let pc = cpu.pc();
        let status = cpu.cpsr();
        mem.write16(KEYCNT, 0x4001).unwrap();
        mem.write16(IE, IRQ).unwrap();
        mem.write16(IME, 1).unwrap();
        press(&mut mem, 1);
        let mut m = Machine::new(cpu, mem);
        assert_eq!(m.step().unwrap(), StepKind::IrqEntry);
        assert_eq!(m.cpu().mode(), Mode::Irq);
        assert_eq!(m.cpu().spsr(), Some(status));
        assert_eq!(m.cpu().registers()[14], pc + 4);
        assert!(pending(m.memory()));
        ack(m.memory_mut());
        m.step().unwrap();
        assert_eq!(m.cpu().pc(), pc);
        assert_eq!(m.cpu().cpsr(), status);
        assert_eq!(
            m.cpu().instruction_set(),
            if thumb {
                InstructionSet::Thumb
            } else {
                InstructionSet::Arm
            }
        );
        m.step().unwrap();
        assert_eq!(m.cpu().registers()[1], 7);
    }
}

fn prepared(instruction: u32, destination: u32, value: u32) -> Machine {
    let mut mem = Memory::new(words(&[
        0xe59f_0008,
        0xe59f_1008,
        instruction,
        0xeaff_fffe,
        destination,
        value,
    ]))
    .unwrap();
    let mut cpu = Cpu::new(ROM_START);
    cpu.step(&mut mem).unwrap();
    cpu.step(&mut mem).unwrap();
    Machine::new(cpu, mem)
}

#[test]
fn cpu_store_samples_keys_then_irq_enters_on_the_following_step() {
    for (instruction, address, value) in [
        (0xe1c0_10b0, KEYCNT, 0x4001),
        (0xe580_1000, KEYINPUT, 0x4001_ffff),
    ] {
        let mut m = prepared(instruction, address, value);
        press(m.memory_mut(), 1);
        m.memory_mut().write16(IE, IRQ).unwrap();
        m.memory_mut().write16(IME, 1).unwrap();
        assert_eq!(m.step().unwrap(), StepKind::Instruction);
        assert_eq!(m.cpu().pc(), ROM_START + 12);
        assert_eq!(m.memory().read16(KEYCNT).unwrap(), 0x4001);
        assert_eq!(m.memory().read16(KEYINPUT).unwrap(), 0x3fe);
        assert_eq!(m.step().unwrap(), StepKind::IrqEntry);
    }
}

#[test]
fn failed_block_store_does_not_commit_key_control_irq_history_or_halt_wake() {
    // STMIA r0!,{r1,r2}: keypad word is valid, but RCNT's upper padding is not mapped.
    let mut m = prepared(0xe8a0_0006, KEYINPUT, 0x4001_0000);
    press(m.memory_mut(), 1);
    m.memory_mut().write16(IE, IRQ).unwrap();
    let before = m.cpu().clone();
    let cycles = m.cycles();
    let timing = m.last_timing();
    assert_eq!(
        m.step(),
        Err(MachineError::Cpu(CpuError::Memory(MemoryError::Unmapped(
            KEYINPUT + 6
        ))))
    );
    assert_eq!(m.cpu(), &before);
    assert_eq!(m.cycles(), cycles);
    assert_eq!(m.last_timing(), timing);
    assert_eq!(m.memory().read16(KEYCNT).unwrap(), 0);
    assert!(!pending(m.memory()));
    m.memory_mut().write8(HALTCNT, 0).unwrap();
    assert!(m.halted());
    m.memory_mut().write16(KEYCNT, 0xc001).unwrap();
    assert!(pending(m.memory())); // Failure did not consume the first AND sample.
    assert!(!m.halted());
}

#[test]
fn dma_control_write_samples_once_and_ready_dma_precedes_irq() {
    for word in [false, true] {
        let mut m = machine(false);
        press(m.memory_mut(), 1);
        m.memory_mut().write16(IE, IRQ).unwrap();
        m.memory_mut().write16(IME, 1).unwrap();
        let value = if word { 0x4001_ffff } else { 0x4001 };
        m.memory_mut().write32(0x0200_0000, value).unwrap();
        m.memory_mut().write32(DMA_BASE, 0x0200_0000).unwrap();
        m.memory_mut()
            .write32(DMA_BASE + 4, if word { KEYINPUT } else { KEYCNT })
            .unwrap();
        // Fixed destination, two units. IRQ from first unit must wait for second.
        m.memory_mut()
            .write32(DMA_BASE + 8, if word { 0x8440_0002 } else { 0x8040_0002 })
            .unwrap();
        let before = m.cpu().clone();
        assert_eq!(m.step().unwrap(), StepKind::Dma { channel: 0 });
        assert!(pending(m.memory()));
        assert_eq!(m.cpu(), &before);
        assert_eq!(m.step().unwrap(), StepKind::Dma { channel: 0 });
        assert_eq!(m.cpu(), &before);
        assert_eq!(m.step().unwrap(), StepKind::IrqEntry);
    }
}

#[test]
fn keypad_and_timer_requests_coexist_and_acknowledge_independently() {
    let mut m = bus();
    m.write32(TIMER_BASE, 0x00c0_ffff).unwrap();
    m.advance_cycles(1);
    m.write16(TIMER_BASE + 2, 0).unwrap();
    m.write16(KEYCNT, 0x4001).unwrap();
    press(&mut m, 1);
    assert_eq!(m.read16(IF).unwrap(), IRQ | 8);
    ack(&mut m);
    assert_eq!(m.read16(IF).unwrap(), 8);
    press(&mut m, 1);
    m.write16(IF, 8).unwrap();
    assert_eq!(m.read16(IF).unwrap(), IRQ);
}

#[test]
fn original_bios_intrwait_can_sleep_until_keypad_callback_records_the_request() {
    let rom = words(&[0xe3a0_0001, 0xe3a0_1a01, 0xef04_0000, 0xeaff_fffe]);
    let mut m = bios::boot(rom).unwrap();
    for _ in 0..100 {
        if m.cpu().pc() == ROM_START + 8 {
            break;
        }
        m.step().unwrap();
    }
    assert_eq!(m.cpu().pc(), ROM_START + 8);
    let before = m.cpu().clone();
    let callback = words(&[
        0xe280_0c02,
        0xe1d0_10b2,
        0xe1c0_10b2,
        0xe59f_2010,
        0xe1d2_30b0,
        0xe183_3001,
        0xe1c2_30b0,
        0xe12f_ff1e,
        0xe1a0_0000,
        bios::IRQ_FLAGS,
    ]);
    for (i, byte) in callback.into_iter().enumerate() {
        m.memory_mut().write8(0x0300_1000 + i as u32, byte).unwrap();
    }
    m.memory_mut()
        .write32(bios::IRQ_HANDLER, 0x0300_1000)
        .unwrap();
    m.memory_mut().write16(KEYCNT, 0x4001).unwrap();
    m.memory_mut().write16(IE, IRQ).unwrap();
    for _ in 0..200 {
        if m.halted() {
            break;
        }
        m.step().unwrap();
    }
    assert!(m.halted());
    press(m.memory_mut(), 1);
    assert!(!m.halted());
    let mut irq_entries = 0;
    for _ in 0..300 {
        if m.cpu().pc() == ROM_START + 12 {
            break;
        }
        if m.step().unwrap() == StepKind::IrqEntry {
            irq_entries += 1;
        }
    }
    assert_eq!(irq_entries, 1);
    assert_eq!(m.cpu().pc(), ROM_START + 12);
    assert_eq!(m.cpu().cpsr(), before.cpsr());
    assert_eq!(&m.cpu().registers()[..15], &before.registers()[..15]);
    assert_eq!(m.memory().read16(IF).unwrap(), 0);
    assert_eq!(m.memory().read16(bios::IRQ_FLAGS).unwrap(), 0);
}
