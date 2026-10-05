use gba_core::{
    cpu::{Cpu, InstructionSet, Mode},
    display::{CYCLES_PER_FRAME, CYCLES_PER_LINE, HBLANK_START, VBLANK_START},
    dma::{DmaError, DMA_BASE},
    io::{DISPSTAT, HALTCNT, IE, IF, IME, POSTFLG, TIMER_BASE},
    machine::{FrameRunError, Machine, MachineError, StepKind},
    memory::{Memory, MemoryError, BIOS_SIZE, ROM_START},
    timing::StepTiming,
};

fn words(code: &[u32]) -> Vec<u8> {
    code.iter().flat_map(|word| word.to_le_bytes()).collect()
}

fn machine() -> Machine {
    Machine::new(
        Cpu::new(ROM_START),
        Memory::new(words(&[0xe280_0001, 0xeaff_fffe])).unwrap(),
    )
}

fn pending_timer(bus: &mut Memory) {
    bus.write32(TIMER_BASE, 0x00c0_ffff).unwrap();
    bus.advance_cycles(1);
    bus.write16(TIMER_BASE + 2, 0).unwrap();
}

fn halt(machine: &mut Machine) {
    // Bare bus writes are explicit host/debug setup, not CPU-originated accesses.
    machine.memory_mut().write8(HALTCNT, 0).unwrap();
}

fn dma(bus: &mut Memory, control: u16, count: u16) {
    bus.write32(DMA_BASE, 0x0200_0000).unwrap();
    bus.write32(DMA_BASE + 4, 0x0300_0000).unwrap();
    bus.write32(
        DMA_BASE + 8,
        u32::from(control | 0x8000) << 16 | u32::from(count),
    )
    .unwrap();
}

// Prepare a real store at entry+8 with operands loaded by CPU instructions.
fn store_machine(
    entry: u32,
    instruction: u32,
    address: u32,
    value: u32,
    timed_setup: bool,
) -> Machine {
    let program = words(&[
        0xe59f_1008,
        0xe59f_0008,
        instruction,
        0xe282_2001,
        address,
        value,
    ]);
    let mut bios = vec![0; BIOS_SIZE];
    let mut bus = if entry < BIOS_SIZE as u32 {
        bios[entry as usize..entry as usize + program.len()].copy_from_slice(&program);
        Memory::with_bios(vec![], bios).unwrap()
    } else if entry == ROM_START {
        Memory::with_bios(program, bios).unwrap()
    } else {
        let mut bus = Memory::with_bios(vec![], bios).unwrap();
        for (offset, value) in program.into_iter().enumerate() {
            bus.write8(entry + offset as u32, value).unwrap();
        }
        bus
    };
    let mut cpu = Cpu::new(entry);
    for _ in 0..2 {
        if timed_setup {
            cpu.step_timed(&mut bus).unwrap();
        } else {
            cpu.step(&mut bus).unwrap();
        }
    }
    Machine::new(cpu, bus)
}

#[test]
fn reset_register_masks_widths_and_reserved_bytes() {
    let mut bus = Memory::new(vec![]).unwrap();
    assert!(!bus.halted());
    assert_eq!(bus.read32(POSTFLG).unwrap(), 0);
    bus.write8(POSTFLG, 0xff).unwrap();
    assert_eq!(bus.read32(POSTFLG).unwrap(), 1);
    assert!(!bus.halted());
    bus.write8(POSTFLG, 0xfe).unwrap();
    assert_eq!(bus.read8(POSTFLG).unwrap(), 0);
    bus.write16(POSTFLG + 2, 0xffff).unwrap();
    assert_eq!(bus.read16(POSTFLG + 2).unwrap(), 0);
    assert!(!bus.halted());
    bus.write16(POSTFLG, 0x7f01).unwrap();
    assert!(bus.halted());
    assert_eq!(bus.read16(POSTFLG).unwrap(), 1); // HALTCNT is write-only.
    assert_eq!(bus.read16(HALTCNT), Err(MemoryError::Unaligned(HALTCNT)));
    assert_eq!(
        bus.read8(POSTFLG + 4),
        Err(MemoryError::Unmapped(POSTFLG + 4))
    );
    assert_eq!(
        bus.read8(POSTFLG + 0x400),
        Err(MemoryError::Unmapped(POSTFLG + 0x400))
    );
}

#[test]
fn all_halt_values_ignore_low_seven_bits() {
    for value in 0..=0x7f {
        let mut machine = machine();
        machine.memory_mut().write8(HALTCNT, value).unwrap();
        assert!(machine.halted());
        assert_eq!(machine.memory().read8(HALTCNT).unwrap(), 0);
        assert_eq!(machine.cycles(), 0);
    }
}

#[test]
fn halt_idle_does_not_fetch_instructions_or_change_cpu_registers() {
    let mut machine = Machine::new(Cpu::new(0xdead_beec), Memory::new(vec![]).unwrap());
    halt(&mut machine);
    let before = machine.cpu().clone();
    for cycles in [HBLANK_START, CYCLES_PER_LINE - HBLANK_START, HBLANK_START] {
        assert_eq!(machine.step().unwrap(), StepKind::HaltIdle);
        assert_eq!(machine.cpu(), &before);
        assert_eq!(
            machine.last_timing(),
            StepTiming {
                idle_cycles: cycles,
                ..StepTiming::default()
            }
        );
        assert!(machine.halted());
    }
    assert_eq!(machine.cycles(), u64::from(CYCLES_PER_LINE + HBLANK_START));
}

#[test]
fn pending_enabled_request_prevents_sleep_regardless_of_ime() {
    for ime in [0, 1] {
        let mut machine = machine();
        pending_timer(machine.memory_mut());
        machine.memory_mut().write16(IE, 8).unwrap();
        machine.memory_mut().write16(IME, ime).unwrap();
        halt(&mut machine);
        assert!(!machine.halted());
        assert_eq!(
            machine.step().unwrap(),
            if ime == 0 {
                StepKind::Instruction
            } else {
                StepKind::IrqEntry
            }
        );
        assert_eq!(machine.memory().read16(IF).unwrap(), 8);
    }
}

#[test]
fn ie_gates_wake_but_ime_does_not_and_wake_is_not_reversed_by_acknowledgement() {
    let mut machine = machine();
    pending_timer(machine.memory_mut());
    machine.memory_mut().write16(IE, 1).unwrap(); // Wrong source.
    halt(&mut machine);
    assert!(machine.halted());
    machine.memory_mut().write16(IME, 1).unwrap();
    assert!(machine.halted());
    machine.memory_mut().write16(IE, 8).unwrap();
    assert!(!machine.halted());
    machine.memory_mut().write16(IF, 8).unwrap();
    machine.memory_mut().write16(IE, 0).unwrap();
    assert!(!machine.halted());
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
}

#[test]
fn timer_wakes_at_exact_overflow_without_ime_and_resumes_next_instruction() {
    let mut machine = machine();
    machine
        .memory_mut()
        .write32(TIMER_BASE, 0x00c0_fff6)
        .unwrap();
    machine.memory_mut().write16(IE, 8).unwrap();
    halt(&mut machine);
    let before = machine.cpu().clone();
    assert_eq!(machine.step().unwrap(), StepKind::HaltIdle);
    assert_eq!(machine.last_timing().idle_cycles, 10);
    assert_eq!(machine.cpu(), &before);
    assert_eq!(machine.cycles(), 10);
    assert!(!machine.halted());
    assert!(!machine.memory().irq_pending());
    assert_eq!(machine.memory().read16(IF).unwrap(), 8);
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    assert_eq!(machine.cpu().registers()[0], 1);
    assert_eq!(machine.cpu().pc(), ROM_START + 4);
}

#[test]
fn cpsr_irq_mask_blocks_handler_but_not_halt_wake() {
    let mut bios = vec![0; BIOS_SIZE];
    bios[0..4].copy_from_slice(&0xe280_0001_u32.to_le_bytes()); // ADD r0,r0,#1
    let mut bus = Memory::with_bios(vec![], bios).unwrap();
    bus.write32(TIMER_BASE, 0x00c0_ffff).unwrap();
    bus.write16(IE, 8).unwrap();
    bus.write16(IME, 1).unwrap();
    bus.write8(HALTCNT, 0).unwrap();
    let mut machine = Machine::new(Cpu::at_reset(), bus);
    assert_eq!(machine.step().unwrap(), StepKind::HaltIdle);
    assert!(!machine.halted());
    assert!(machine.memory().irq_pending());
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    assert_eq!(machine.cpu().mode(), Mode::Supervisor);
    assert_eq!(machine.cpu().registers()[0], 1);
}

#[test]
fn unmasked_wake_enters_irq_before_executing_resumed_instruction() {
    let mut machine = machine();
    machine
        .memory_mut()
        .write32(TIMER_BASE, 0x00c0_ffff)
        .unwrap();
    machine.memory_mut().write16(IE, 8).unwrap();
    machine.memory_mut().write16(IME, 1).unwrap();
    halt(&mut machine);
    assert_eq!(machine.step().unwrap(), StepKind::HaltIdle);
    assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
    assert_eq!(machine.cpu().mode(), Mode::Irq);
    assert_eq!(machine.cpu().pc(), 0x18);
    assert_eq!(machine.cpu().registers()[14], ROM_START + 4);
    assert_eq!(machine.cpu().registers()[0], 0);
}

#[test]
fn wake_irq_returns_to_thumb_without_skipping_instruction() {
    let mut rom = words(&[0xe28f_0001, 0xe12f_ff10]); // ADD r0,pc,#1; BX r0
    rom.extend(0x2107_u16.to_le_bytes()); // MOV r1,#7
    let bios = 0xe25e_f004_u32.to_le_bytes().repeat(BIOS_SIZE / 4); // SUBS pc,lr,#4
    let mut bus = Memory::with_bios(rom, bios).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    cpu.step(&mut bus).unwrap();
    cpu.step(&mut bus).unwrap();
    bus.write32(TIMER_BASE, 0x00c0_ffff).unwrap();
    bus.write16(IE, 8).unwrap();
    bus.write16(IME, 1).unwrap();
    bus.write8(HALTCNT, 0).unwrap();
    let mut machine = Machine::new(cpu, bus);
    assert_eq!(machine.step().unwrap(), StepKind::HaltIdle);
    assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
    assert_eq!(machine.cpu().spsr(), Some(0x3f));
    machine.memory_mut().write16(TIMER_BASE + 2, 0).unwrap();
    machine.memory_mut().write16(IF, 8).unwrap();
    machine.step().unwrap();
    assert_eq!(machine.cpu().instruction_set(), InstructionSet::Thumb);
    assert_eq!(machine.cpu().pc(), ROM_START + 8);
    machine.step().unwrap();
    assert_eq!(machine.cpu().registers()[1], 7);
}

#[test]
fn all_display_interrupt_sources_can_wake_halt_at_their_edges() {
    for (control, mask, edge) in [
        (8, 1, VBLANK_START),
        (0x10, 2, HBLANK_START),
        (0x0120, 4, CYCLES_PER_LINE),
    ] {
        let mut machine = machine();
        machine.memory_mut().write16(DISPSTAT, control).unwrap();
        machine.memory_mut().write16(IE, mask).unwrap();
        machine.memory_mut().advance_cycles(edge - 1);
        halt(&mut machine);
        assert_eq!(machine.step().unwrap(), StepKind::HaltIdle);
        assert_eq!(machine.last_timing().idle_cycles, 1);
        assert_eq!(machine.cycles(), u64::from(edge));
        assert!(!machine.halted());
        assert_eq!(machine.memory().read16(IF).unwrap(), mask);
    }
}

#[test]
fn local_interrupt_enable_is_required_to_generate_a_wake_request() {
    let mut machine = machine();
    machine
        .memory_mut()
        .write32(TIMER_BASE, 0x0080_ffff)
        .unwrap(); // IRQ off.
    machine.memory_mut().write16(IE, 0x3fff).unwrap();
    halt(&mut machine);
    assert_eq!(machine.step().unwrap(), StepKind::HaltIdle);
    assert_eq!(machine.last_timing().idle_cycles, 1);
    assert_eq!(machine.memory().read16(IF).unwrap(), 0);
    assert!(machine.halted());
}

#[test]
fn idle_scheduling_accounts_for_timer_prescaler_phase() {
    for (selection, divisor) in [1, 64, 256, 1024].into_iter().enumerate() {
        let mut machine = machine();
        machine
            .memory_mut()
            .write32(TIMER_BASE, ((0xc0 | selection as u32) << 16) | 0xffff)
            .unwrap();
        machine.memory_mut().write16(IE, 8).unwrap();
        machine.memory_mut().advance_cycles(divisor - 1);
        halt(&mut machine);
        assert_eq!(machine.step().unwrap(), StepKind::HaltIdle);
        assert_eq!(machine.last_timing().idle_cycles, 1);
        assert_eq!(machine.cycles(), u64::from(divisor));
        assert!(!machine.halted());
    }
}

#[test]
fn cascaded_timer_wakes_only_on_predecessor_overflow() {
    let mut machine = machine();
    machine
        .memory_mut()
        .write32(TIMER_BASE, 0x0080_fff6)
        .unwrap(); // Ten cycles per overflow, IRQ off.
    machine
        .memory_mut()
        .write32(TIMER_BASE + 4, 0x00c4_fffe)
        .unwrap(); // Two pulses, IRQ on.
    machine.memory_mut().write16(IE, 0x10).unwrap();
    halt(&mut machine);
    for expected in [true, false] {
        assert_eq!(machine.step().unwrap(), StepKind::HaltIdle);
        assert_eq!(machine.last_timing().idle_cycles, 10);
        assert_eq!(machine.halted(), expected);
    }
    assert_eq!(machine.memory().read16(IF).unwrap(), 0x10);
    assert_eq!(machine.cycles(), 20);
}

#[test]
fn disabled_predecessor_cannot_clock_a_cascaded_timer_during_halt() {
    let mut machine = machine();
    machine
        .memory_mut()
        .write32(TIMER_BASE + 4, 0x00c4_ffff)
        .unwrap();
    machine.memory_mut().write16(IE, 0x10).unwrap();
    halt(&mut machine);
    for _ in 0..4 {
        assert_eq!(machine.step().unwrap(), StepKind::HaltIdle);
        assert!(machine.halted());
        assert_eq!(machine.memory().read16(TIMER_BASE + 4).unwrap(), 0xffff);
    }
}

#[test]
fn idle_batches_match_cycle_by_cycle_display_and_timer_execution() {
    for seed in 0..16_u32 {
        let configure = || {
            let mut bus = Memory::new(vec![]).unwrap();
            bus.write16(DISPSTAT, ((seed as u16 * 17) << 8) | 0x38)
                .unwrap();
            for index in 0..4 {
                let control = if index != 0 && (seed + index) % 3 == 0 {
                    0xc4
                } else {
                    0xc0 | ((seed + index) & 3)
                };
                bus.write32(
                    TIMER_BASE + index * 4,
                    control << 16 | (0xffff - (seed * 13 + index * 7)),
                )
                .unwrap();
            }
            bus.advance_cycles(seed * 91);
            bus.write16(IF, 0x3fff).unwrap();
            bus.write8(HALTCNT, 0).unwrap(); // IE=0, so all events are observable without wake.
            bus
        };
        let mut reference = configure();
        let mut machine = Machine::new(Cpu::new(ROM_START), configure());
        for _ in 0..40 {
            machine.memory_mut().write16(IF, 0x3fff).unwrap();
            reference.write16(IF, 0x3fff).unwrap();
            let mut elapsed = 0;
            loop {
                reference.advance_cycles(1);
                elapsed += 1;
                let position = reference.display_position();
                if position.line_cycle == 0
                    || u32::from(position.line_cycle) == HBLANK_START
                    || reference.read16(IF).unwrap() != 0
                {
                    break;
                }
                assert!(elapsed < CYCLES_PER_LINE);
            }
            assert_eq!(machine.step().unwrap(), StepKind::HaltIdle);
            assert_eq!(machine.last_timing().idle_cycles, elapsed);
            assert_eq!(
                machine.memory().display_position(),
                reference.display_position()
            );
            assert_eq!(machine.cycles(), reference.cycles());
            assert_eq!(
                machine.memory().read16(IF).unwrap(),
                reference.read16(IF).unwrap()
            );
            for index in 0..4 {
                assert_eq!(
                    machine.memory().read32(TIMER_BASE + index * 4).unwrap(),
                    reference.read32(TIMER_BASE + index * 4).unwrap()
                );
            }
        }
    }
}

#[test]
fn blank_triggered_dma_continues_during_halt_and_completion_can_wake() {
    let mut machine = machine();
    machine.memory_mut().write16(0x0200_0000, 0x1234).unwrap();
    dma(machine.memory_mut(), 0x5000, 1); // VBlank and completion IRQ.
    machine.memory_mut().write16(IE, 0x100).unwrap();
    halt(&mut machine);
    assert_eq!(machine.run_until_vblank(320).unwrap(), 320);
    assert!(machine.halted()); // No display IRQ enabled; DMA has not run yet.
    assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 0 });
    assert_eq!(machine.memory().read16(0x0300_0000).unwrap(), 0x1234);
    assert_eq!(machine.memory().read16(IF).unwrap(), 0x100);
    assert!(!machine.halted());
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
}

#[test]
fn dma_without_completion_irq_does_not_wake_halt() {
    let mut machine = machine();
    dma(machine.memory_mut(), 0x2000, 1); // HBlank, no IRQ.
    halt(&mut machine);
    assert_eq!(machine.step().unwrap(), StepKind::HaltIdle);
    assert_eq!(machine.cycles(), u64::from(HBLANK_START));
    assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 0 });
    assert!(machine.halted());
    assert_eq!(machine.step().unwrap(), StepKind::HaltIdle);
    assert_eq!(machine.cycles(), u64::from(CYCLES_PER_LINE));
}

#[test]
fn dma_finishes_before_irq_delivery_after_wake_during_transfer() {
    let mut machine = machine();
    dma(machine.memory_mut(), 0, 2);
    machine
        .memory_mut()
        .write32(TIMER_BASE, 0x00c0_ffff)
        .unwrap();
    machine.memory_mut().write16(IE, 8).unwrap();
    machine.memory_mut().write16(IME, 1).unwrap();
    halt(&mut machine);
    assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 0 });
    assert!(!machine.halted());
    assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 0 });
    assert_eq!(machine.cpu().pc(), ROM_START);
    assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
}

#[test]
fn unsupported_dma_power_control_write_preserves_halt_and_clock() {
    let mut machine = machine();
    dma(machine.memory_mut(), 0, 1);
    machine.memory_mut().write16(DMA_BASE + 10, 0).unwrap();
    machine.memory_mut().write32(DMA_BASE + 4, POSTFLG).unwrap();
    machine.memory_mut().write16(DMA_BASE + 10, 0x8000).unwrap();
    halt(&mut machine);
    assert_eq!(
        machine.step(),
        Err(MachineError::Dma(DmaError::PowerControlDestination {
            channel: 0,
            address: POSTFLG
        }))
    );
    assert!(machine.halted());
    assert_eq!(machine.cycles(), 0);
    assert_eq!(machine.memory().read8(POSTFLG).unwrap(), 0);
}

#[test]
fn byte_halfword_and_word_cpu_stores_enter_halt_only_from_bios() {
    for timed in [false, true] {
        for entry in [0x100, ROM_START, 0x0300_0000] {
            for (instruction, address, value) in [
                (0xe5c1_0000, HALTCNT, 0x7f),        // STRB
                (0xe1c1_00b0, POSTFLG, 1),           // STRH
                (0xe581_0000, POSTFLG, 0xffff_0001), // STR
            ] {
                let mut machine = store_machine(entry, instruction, address, value, timed);
                assert_eq!(machine.step().unwrap(), StepKind::Instruction);
                assert_eq!(machine.halted(), entry < BIOS_SIZE as u32);
                assert_eq!(machine.cpu().pc(), entry + 12);
                assert_eq!(machine.cpu().registers()[2], 0);
                let before = machine.cpu().clone();
                if entry < BIOS_SIZE as u32 {
                    assert_eq!(machine.step().unwrap(), StepKind::HaltIdle);
                    assert_eq!(machine.cpu(), &before);
                } else {
                    assert_eq!(machine.memory().read8(POSTFLG).unwrap(), 0);
                    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
                    assert_eq!(machine.cpu().registers()[2], 1);
                }
            }
        }
    }
}

#[test]
fn thumb_halt_stores_use_the_executing_address_for_bios_access() {
    for entry in [0x100, ROM_START] {
        let mut code = words(&[0xe28f_0001, 0xe12f_ff10]); // Enter Thumb at entry+8.
        for instruction in [0x2000_u16, 0x4901, 0x7008, 0x3201] {
            // MOV r0,#0; LDR r1,[pc,#4]; STRB r0,[r1]; ADD r2,#1.
            code.extend(instruction.to_le_bytes());
        }
        code.extend(HALTCNT.to_le_bytes());
        let mut bios = vec![0; BIOS_SIZE];
        let mut bus = if entry < BIOS_SIZE as u32 {
            bios[entry as usize..entry as usize + code.len()].copy_from_slice(&code);
            Memory::with_bios(vec![], bios).unwrap()
        } else {
            Memory::new(code).unwrap()
        };
        let mut cpu = Cpu::new(entry);
        for _ in 0..4 {
            cpu.step(&mut bus).unwrap();
        }
        let mut machine = Machine::new(cpu, bus);
        assert_eq!(machine.cpu().instruction_set(), InstructionSet::Thumb);
        assert_eq!(machine.step().unwrap(), StepKind::Instruction);
        assert_eq!(machine.cpu().pc(), entry + 14);
        assert_eq!(machine.halted(), entry < BIOS_SIZE as u32);
    }
}

#[test]
fn halt_store_finishes_and_can_wake_during_its_own_cycles() {
    let mut machine = store_machine(0x100, 0xe5c1_0000, HALTCNT, 0, false);
    machine
        .memory_mut()
        .write32(TIMER_BASE, 0x00c0_ffff)
        .unwrap();
    machine.memory_mut().write16(IE, 8).unwrap();
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    assert_eq!(machine.cpu().pc(), 0x10c);
    assert_eq!(machine.cycles(), 2);
    assert!(!machine.halted());
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    assert_eq!(machine.cpu().registers()[2], 1);
}

#[test]
fn stop_diagnostics_validate_entire_write_before_postflg_or_halt_changes() {
    let mut bus = Memory::new(vec![]).unwrap();
    for value in [0x80, 0xff] {
        assert_eq!(
            bus.write8(HALTCNT, value),
            Err(MemoryError::UnsupportedStop)
        );
        assert_eq!(
            bus.write16(POSTFLG, u16::from(value) << 8 | 1),
            Err(MemoryError::UnsupportedStop)
        );
        assert_eq!(
            bus.write32(POSTFLG, u32::from(value) << 8 | 1),
            Err(MemoryError::UnsupportedStop)
        );
        assert_eq!(bus.read32(POSTFLG).unwrap(), 0);
        assert!(!bus.halted());
        assert_eq!(bus.cycles(), 0);
    }
}

#[test]
fn bios_stop_error_preserves_cpu_timing_and_clears_access_context() {
    let mut machine = store_machine(0x100, 0xe581_0000, POSTFLG, 0x8001, true);
    let before = machine.cpu().clone();
    assert_eq!(
        machine.step(),
        Err(MachineError::Cpu(MemoryError::UnsupportedStop.into()))
    );
    assert_eq!(machine.cpu(), &before);
    assert_eq!(machine.cycles(), 0);
    assert_eq!(machine.last_timing(), StepTiming::default());
    assert_eq!(machine.memory().read8(POSTFLG).unwrap(), 0);
    assert!(!machine.halted());
    // A failed non-BIOS instruction must also clear its access context.
    let mut machine = store_machine(ROM_START, 0xe581_0000, 0x0400_0058, 0, true);
    assert!(machine.step().is_err());
    machine.memory_mut().write8(POSTFLG, 1).unwrap();
    halt(&mut machine);
    assert_eq!(machine.memory().read8(POSTFLG).unwrap(), 1);
    assert!(machine.halted());
}

#[test]
fn non_bios_stop_write_is_ignored_not_a_diagnostic() {
    for entry in [ROM_START, 0x0300_0000] {
        let mut machine = store_machine(entry, 0xe581_0000, POSTFLG, 0x8001, false);
        assert_eq!(machine.step().unwrap(), StepKind::Instruction);
        assert!(!machine.halted());
        assert_eq!(machine.memory().read32(POSTFLG).unwrap(), 0);
    }
}

#[test]
fn failed_block_store_cannot_partially_halt_or_change_postflg() {
    // The first word is mapped; the second is not. Validation precedes all writes.
    let mut machine = store_machine(0x100, 0xe8a1_0005, POSTFLG, 1, false); // STMIA r1!,{r0,r2}
    let before = machine.cpu().clone();
    assert!(machine.step().is_err());
    assert_eq!(machine.cpu(), &before);
    assert!(!machine.halted());
    assert_eq!(machine.memory().read8(POSTFLG).unwrap(), 0);
    assert_eq!(machine.cycles(), 0);
}

#[test]
fn cpu_only_apis_record_halt_but_do_not_idle_or_enforce_pause() {
    for timed in [false, true] {
        let mut bios = vec![0; BIOS_SIZE];
        bios[..24].copy_from_slice(&words(&[
            0xe59f_1008,
            0xe3a0_0000,
            0xe5c1_0000,
            0xe282_2001,
            HALTCNT,
            0,
        ]));
        let mut bus = Memory::with_bios(vec![], bios).unwrap();
        let mut cpu = Cpu::new(0);
        for _ in 0..3 {
            if timed {
                cpu.step_timed(&mut bus).unwrap();
            } else {
                cpu.step(&mut bus).unwrap();
            }
        }
        assert!(bus.halted());
        cpu.step(&mut bus).unwrap(); // CPU-only API intentionally bypasses machine scheduling.
        assert_eq!(cpu.registers()[2], 1);
        assert_eq!(bus.cycles(), 0);
        assert!(bus.halted());
    }
}

#[test]
fn frame_runner_remains_bounded_with_no_wake_source_and_reports_idle_cost() {
    let mut machine = Machine::new(Cpu::new(ROM_START), Memory::new(vec![]).unwrap());
    halt(&mut machine);
    assert_eq!(
        machine.run_until_vblank(0),
        Err(FrameRunError::StepLimit(0))
    );
    assert_eq!(machine.cycles(), 0);
    assert_eq!(
        machine.run_until_vblank(319),
        Err(FrameRunError::StepLimit(319))
    );
    assert!(machine.halted());
    assert_eq!(machine.run_until_vblank(1).unwrap(), 1);
    assert_eq!(machine.cycles(), u64::from(VBLANK_START));
    assert_eq!(machine.memory().display_position().line_cycle, 0);
    assert!(machine.halted());
    assert_eq!(machine.run_until_vblank(456).unwrap(), 456);
    assert_eq!(machine.cycles(), u64::from(VBLANK_START + CYCLES_PER_FRAME));
}

#[test]
fn externally_advanced_clock_can_wake_halt_without_cpu_execution() {
    let mut machine = machine();
    machine
        .memory_mut()
        .write32(TIMER_BASE, 0x00c0_fff0)
        .unwrap();
    machine.memory_mut().write16(IE, 8).unwrap();
    halt(&mut machine);
    let before = machine.cpu().clone();
    machine.memory_mut().advance_cycles(16);
    assert!(!machine.halted());
    assert_eq!(machine.cpu(), &before);
    assert_eq!(machine.last_timing(), StepTiming::default());
}
