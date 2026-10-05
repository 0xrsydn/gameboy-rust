use gba_core::{
    cpu::{Cpu, CpuError, InstructionSet},
    display::{CYCLES_PER_FRAME, VBLANK_START},
    dma::{DmaError, DMA_BASE},
    input::Buttons,
    io::{DISPCNT, HALTCNT, IE, IF, IME, KEYCNT, POSTFLG, TIMER_BASE},
    machine::{FrameRunError, Machine, MachineError, StepKind},
    memory::{Memory, MemoryError, BIOS_SIZE, ROM_START},
    timing::StepTiming,
    video::Framebuffer,
};

const KEY_IRQ: u16 = 1 << 12;

fn words(code: &[u32]) -> Vec<u8> {
    code.iter().flat_map(|word| word.to_le_bytes()).collect()
}

fn machine(masked: bool) -> Machine {
    let mut bios = vec![0; BIOS_SIZE];
    bios[..12].copy_from_slice(&words(&[
        if masked { 0xe321_f09f } else { 0xe321_f01f },
        0xe280_0001,
        0xeaff_fffe,
    ]));
    let mut m = Machine::new(Cpu::new(0), Memory::with_bios(vec![], bios).unwrap());
    m.step().unwrap();
    m
}

fn enable_keypad(m: &mut Memory) {
    m.write16(KEYCNT, 0x4001).unwrap();
    m.write16(IE, KEY_IRQ).unwrap();
}

fn stop(m: &mut Memory) {
    m.write8(HALTCNT, 0x80).unwrap();
}

fn press(m: &mut Memory, buttons: u16) {
    m.set_buttons(Buttons::from_bits(buttons));
}

fn pending_timer(m: &mut Memory) {
    m.write32(TIMER_BASE, 0x00c0_ffff).unwrap();
    m.advance_cycles(1);
    m.write16(TIMER_BASE + 2, 0).unwrap();
    assert_ne!(m.read16(IF).unwrap() & 8, 0);
}

#[test]
fn stop_idle_does_not_fetch_or_advance_any_clock() {
    let mut m = Machine::new(Cpu::new(0xdead_beec), Memory::new(vec![]).unwrap());
    m.memory_mut().write32(TIMER_BASE, 0x00c0_fff0).unwrap();
    enable_keypad(m.memory_mut());
    stop(m.memory_mut());
    let cpu = m.cpu().clone();
    let position = m.memory().display_position();
    for _ in 0..100 {
        assert_eq!(m.step().unwrap(), StepKind::StopIdle);
        assert_eq!(m.cpu(), &cpu);
        assert_eq!(m.last_timing(), StepTiming::default());
        assert_eq!(m.cycles(), 0);
        assert_eq!(m.memory().display_position(), position);
        assert_eq!(m.memory().read16(TIMER_BASE).unwrap(), 0xfff0);
        assert_eq!(m.memory().read16(IF).unwrap(), 0);
    }
    for cycles in [0, 1, VBLANK_START, CYCLES_PER_FRAME, u32::MAX] {
        m.memory_mut().advance_cycles(cycles);
        assert_eq!(m.cycles(), 0);
        assert_eq!(m.memory().display_position(), position);
        assert_eq!(m.memory().read16(TIMER_BASE).unwrap(), 0xfff0);
    }
    assert!(m.stopped());
    assert!(!m.halted());
}

#[test]
fn keypad_wake_ignores_ime_and_cpu_mask_and_does_not_latch_if() {
    for masked in [false, true] {
        for ie in [0, KEY_IRQ] {
            for ime in [0, 1] {
                let mut m = machine(masked);
                enable_keypad(m.memory_mut());
                m.memory_mut().write16(IE, ie).unwrap();
                m.memory_mut().write16(IME, ime).unwrap();
                stop(m.memory_mut());
                let before = m.cpu().clone();
                let cycles = m.cycles();
                press(m.memory_mut(), 1);
                assert_eq!(m.cpu(), &before);
                assert_eq!(m.cycles(), cycles);
                assert_eq!(m.stopped(), ie == 0);
                assert_eq!(m.memory().read16(IF).unwrap(), 0);
                assert_eq!(
                    m.step().unwrap(),
                    if ie == 0 {
                        StepKind::StopIdle
                    } else {
                        StepKind::Instruction
                    }
                );
            }
        }
    }
}

#[test]
fn live_keypad_wake_uses_enable_selection_and_or_and_conditions() {
    for (control, inputs, wakes) in [
        (0x0001, 1, false),
        (0x8001, 1, false),
        (0x4001, 2, false),
        (0x4003, 2, true),
        (0xc003, 1, false),
        (0xc003, 7, true),
        (0x4000, 0x3ff, false),
        (0xc000, 0, true),
        (0x4100, 0x100, true),
        (0xc200, 0x200, true),
    ] {
        let mut m = machine(false);
        enable_keypad(m.memory_mut());
        stop(m.memory_mut());
        // These are debug/host writes; a stopped CPU cannot execute stores.
        m.memory_mut().write16(KEYCNT, control).unwrap();
        press(m.memory_mut(), inputs);
        assert_eq!(
            m.stopped(),
            !wakes,
            "control={control:#x}, inputs={inputs:#x}"
        );
        // Empty AND can wake on the control write before the later input sample.
        if control != 0xc000 {
            assert_eq!(m.memory().read16(IF).unwrap(), 0);
        }
    }
}

#[test]
fn stale_keypad_and_timer_if_flags_cannot_wake_stop() {
    let mut m = machine(false);
    pending_timer(m.memory_mut());
    enable_keypad(m.memory_mut());
    press(m.memory_mut(), 1);
    press(m.memory_mut(), 0);
    m.memory_mut().write16(IE, KEY_IRQ | 8).unwrap();
    m.memory_mut().write16(IME, 1).unwrap();
    assert_eq!(m.memory().read16(IF).unwrap(), KEY_IRQ | 8);
    stop(m.memory_mut());
    assert!(m.stopped());
    assert_eq!(m.step().unwrap(), StepKind::StopIdle);
    press(m.memory_mut(), 1);
    assert!(!m.stopped());
    assert_eq!(m.memory().read16(IF).unwrap(), KEY_IRQ | 8);
    assert_eq!(m.step().unwrap(), StepKind::IrqEntry); // Old pending request, not a STOP-generated IF.
}

#[test]
fn already_held_matching_keys_prevent_stop_even_after_and_request_was_acknowledged() {
    let mut m = machine(false);
    m.memory_mut().write16(KEYCNT, 0xc003).unwrap();
    m.memory_mut().write16(IE, KEY_IRQ).unwrap();
    press(m.memory_mut(), 3);
    m.memory_mut().write16(IF, KEY_IRQ).unwrap();
    press(m.memory_mut(), 3); // AND history suppresses another IF request.
    assert_eq!(m.memory().read16(IF).unwrap(), 0);
    stop(m.memory_mut());
    assert!(!m.stopped()); // Wake depends on live keys, not AND polling history.
    assert_eq!(m.memory().read16(IF).unwrap(), 0);
}

#[test]
fn enabling_ie_can_wake_held_keys_without_if_and_release_does_not_reenter_stop() {
    let mut m = machine(false);
    m.memory_mut().write16(KEYCNT, 0x4001).unwrap();
    stop(m.memory_mut());
    press(m.memory_mut(), 1);
    assert!(m.stopped());
    m.memory_mut().write16(IE, KEY_IRQ).unwrap();
    assert!(!m.stopped());
    assert_eq!(m.memory().read16(IF).unwrap(), 0);
    press(m.memory_mut(), 0);
    assert!(!m.stopped());
    assert_eq!(m.step().unwrap(), StepKind::Instruction);
}

#[test]
fn stopped_input_does_not_consume_the_first_running_and_sample() {
    let mut m = machine(false);
    m.memory_mut().write16(KEYCNT, 0xc001).unwrap();
    m.memory_mut().write16(IE, KEY_IRQ).unwrap();
    stop(m.memory_mut());
    press(m.memory_mut(), 1);
    assert!(!m.stopped());
    assert_eq!(m.memory().read16(IF).unwrap(), 0);
    press(m.memory_mut(), 1);
    assert_eq!(m.memory().read16(IF).unwrap(), KEY_IRQ);
}

#[test]
fn dma_is_frozen_before_work_or_diagnostics_and_resumes_before_cpu_after_wake() {
    for invalid in [false, true] {
        let mut m = machine(false);
        enable_keypad(m.memory_mut());
        m.memory_mut().write32(0x0200_0000, 0x1234_5678).unwrap();
        m.memory_mut().write32(DMA_BASE, 0x0200_0000).unwrap();
        m.memory_mut().write32(DMA_BASE + 4, 0x0300_0000).unwrap();
        m.memory_mut()
            .write32(
                DMA_BASE + 8,
                if invalid { 0xb400_0001 } else { 0x8400_0001 },
            )
            .unwrap();
        stop(m.memory_mut());
        let cpu = m.cpu().clone();
        let cycles = m.cycles();
        for _ in 0..4 {
            assert_eq!(m.step().unwrap(), StepKind::StopIdle);
        }
        assert_eq!(m.cpu(), &cpu);
        assert_eq!(m.cycles(), cycles);
        assert_eq!(m.memory().read32(0x0300_0000).unwrap(), 0);
        press(m.memory_mut(), 1);
        if invalid {
            assert_eq!(
                m.step(),
                Err(MachineError::Dma(DmaError::UnsupportedControl {
                    channel: 0,
                    control: 0xb400
                }))
            );
        } else {
            assert_eq!(m.step().unwrap(), StepKind::Dma { channel: 0 });
            assert_eq!(m.cpu(), &cpu);
            assert_eq!(m.memory().read32(0x0300_0000).unwrap(), 0x1234_5678);
            assert_eq!(m.step().unwrap(), StepKind::Instruction);
        }
    }
}

#[test]
fn stopped_clock_keeps_timer_phase_display_phase_and_video_memory() {
    let mut frozen = Memory::new(vec![]).unwrap();
    let mut reference = Memory::new(vec![]).unwrap();
    for m in [&mut frozen, &mut reference] {
        enable_keypad(m);
        m.write32(TIMER_BASE, 0x0081_1000).unwrap(); // /64, with a partial prescaler period.
        m.write16(DISPCNT, 0x80).unwrap();
        m.write32(0x0600_0000, 0x1234_5678).unwrap();
        m.advance_cycles(63);
    }
    stop(&mut frozen);
    frozen.advance_cycles(u32::MAX);
    press(&mut frozen, 1);
    assert!(!frozen.stopped());
    assert_eq!(frozen.read16(IF).unwrap(), 0);
    for cycles in [1, 63, 1, 1000, CYCLES_PER_FRAME] {
        frozen.advance_cycles(cycles);
        reference.advance_cycles(cycles);
        assert_eq!(frozen.cycles(), reference.cycles());
        assert_eq!(frozen.display_position(), reference.display_position());
        assert_eq!(
            frozen.read16(TIMER_BASE).unwrap(),
            reference.read16(TIMER_BASE).unwrap()
        );
    }
    assert_eq!(frozen.read32(0x0600_0000).unwrap(), 0x1234_5678);
}

#[test]
fn frame_runner_reports_stop_without_spinning_and_can_resume_after_input() {
    let mut m = machine(false);
    enable_keypad(m.memory_mut());
    stop(m.memory_mut());
    let cycles = m.cycles();
    assert_eq!(m.run_until_vblank(0), Err(FrameRunError::StepLimit(0)));
    assert_eq!(m.run_until_vblank(usize::MAX), Err(FrameRunError::Stopped));
    assert_eq!(m.cycles(), cycles);
    press(m.memory_mut(), 1);
    assert!(m.run_until_vblank(100_000).is_ok());
    assert_eq!(m.memory().display_position().vblanks, 1);
}

#[test]
fn stop_preserves_a_completed_captured_frame() {
    let mut m = machine(false);
    enable_keypad(m.memory_mut());
    m.memory_mut().write16(DISPCNT, 0x80).unwrap();
    m.memory_mut().set_scanline_rendering(true);
    m.memory_mut().advance_cycles(CYCLES_PER_FRAME * 2);
    let mut before = Framebuffer::default();
    assert!(m.memory().present_frame(&mut before).unwrap());
    let position = m.memory().display_position();
    stop(m.memory_mut());
    for _ in 0..10 {
        assert_eq!(m.step().unwrap(), StepKind::StopIdle);
    }
    m.memory_mut().advance_cycles(CYCLES_PER_FRAME);
    let mut after = Framebuffer::default();
    assert!(m.memory().present_frame(&mut after).unwrap());
    assert_eq!(m.memory().display_position(), position);
    assert_eq!(before.pixels(), after.pixels());
}

#[test]
fn arm_and_thumb_stop_stores_finish_with_normal_cycles_before_freezing() {
    for thumb in [false, true] {
        let entry = 0x100;
        let (program, setup_steps, next_pc) = if thumb {
            let mut bytes = words(&[0xe28f_0001, 0xe12f_ff10]);
            for op in [0x2080_u16, 0x4901, 0x7008, 0x3201] {
                bytes.extend(op.to_le_bytes());
            }
            bytes.extend(HALTCNT.to_le_bytes());
            (bytes, 4, entry + 14)
        } else {
            (
                words(&[0xe59f_1008, 0xe3a0_0080, 0xe5c1_0000, 0xe282_2001, HALTCNT]),
                2,
                entry + 12,
            )
        };
        let mut bios = vec![0; BIOS_SIZE];
        bios[entry as usize..entry as usize + program.len()].copy_from_slice(&program);
        let mut mem = Memory::with_bios(vec![], bios).unwrap();
        let mut cpu = Cpu::new(entry);
        for _ in 0..setup_steps {
            cpu.step(&mut mem).unwrap();
        }
        enable_keypad(&mut mem);
        mem.write32(TIMER_BASE, 0x00c0_ffff).unwrap();
        let mut m = Machine::new(cpu, mem);
        assert_eq!(m.step().unwrap(), StepKind::Instruction);
        assert_eq!(m.cpu().pc(), next_pc);
        assert_eq!(
            m.cpu().instruction_set(),
            if thumb {
                InstructionSet::Thumb
            } else {
                InstructionSet::Arm
            }
        );
        assert!(m.stopped());
        assert_eq!(m.cycles(), u64::from(m.last_timing().total()));
        assert!(m.cycles() > 0);
        assert_eq!(m.memory().read16(IF).unwrap(), 8); // Last instruction's clocks completed.
        let cpu = m.cpu().clone();
        assert_eq!(m.step().unwrap(), StepKind::StopIdle);
        assert_eq!(m.cpu(), &cpu);
        press(m.memory_mut(), 1);
        assert_eq!(m.step().unwrap(), StepKind::Instruction);
        assert_eq!(m.cpu().registers()[2], 1);
    }
}

#[test]
fn failed_block_store_cannot_partially_enter_stop_or_set_postflg() {
    let mut bios = vec![0; BIOS_SIZE];
    bios[..24].copy_from_slice(&words(&[
        0xe59f_0008,
        0xe59f_1008,
        0xe8a0_0006,
        0xeaff_fffe,
        POSTFLG,
        0x8001,
    ]));
    let mut mem = Memory::with_bios(vec![], bios).unwrap();
    let mut cpu = Cpu::new(0);
    cpu.step(&mut mem).unwrap();
    cpu.step(&mut mem).unwrap();
    let mut m = Machine::new(cpu, mem);
    let before = m.cpu().clone();
    assert_eq!(
        m.step(),
        Err(MachineError::Cpu(CpuError::Memory(MemoryError::Unmapped(
            POSTFLG + 4
        ))))
    );
    assert_eq!(m.cpu(), &before);
    assert!(!m.stopped());
    assert!(!m.halted());
    assert_eq!(m.memory().read8(POSTFLG).unwrap(), 0);
    assert_eq!(m.cycles(), 0);
    assert_eq!(m.last_timing(), StepTiming::default());
}

#[test]
fn non_bios_thumb_stop_writes_are_ignored() {
    let mut rom = words(&[0xe28f_0001, 0xe12f_ff10]);
    for op in [0x2080_u16, 0x4901, 0x7008, 0x3201] {
        rom.extend(op.to_le_bytes());
    }
    rom.extend(HALTCNT.to_le_bytes());
    let mut m = Machine::new(Cpu::new(ROM_START), Memory::new(rom).unwrap());
    for _ in 0..6 {
        assert_eq!(m.step().unwrap(), StepKind::Instruction);
    }
    assert!(!m.stopped());
    assert!(!m.halted());
    assert_eq!(m.cpu().registers()[2], 1);
}
