use gba_core::{
    cpu::{Cpu, CpuError, InstructionSet, Mode},
    io::{IE, IF, IME, TIMER_BASE, WAITCNT},
    machine::{Machine, MachineError, StepKind},
    memory::{Memory, MemoryError, BIOS_SIZE, ROM_START},
};
use gba_demos::timer_demo::{timer_demo, TIMER_DEMO_STEPS};

fn words(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn timer(bus: &mut Memory) {
    bus.write32(TIMER_BASE, 0x00c0_ffff).unwrap();
    bus.write16(IE, 8).unwrap();
    bus.write32(IME, 1).unwrap();
}

fn pending_timer(bus: &mut Memory) {
    timer(bus);
    bus.advance_cycles(1);
    bus.write16(TIMER_BASE + 2, 0).unwrap();
}

// Load test operands through real instructions, without exposing CPU setters.
fn prepared(instruction: u32, operands: &[(usize, u32)]) -> Machine {
    let offset = (operands.len() as u32 - 1) * 4;
    let mut code: Vec<u32> = operands
        .iter()
        .map(|&(register, _)| 0xe59f_0000 | (register as u32) << 12 | offset)
        .collect();
    code.push(instruction);
    code.extend(operands.iter().map(|&(_, value)| value));
    let mut bus = Memory::new(words(&code)).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    for _ in operands {
        cpu.step(&mut bus).unwrap();
    }
    Machine::new(cpu, bus)
}

#[test]
fn timer_demo_configures_hardware_in_cpu_code_and_handles_exactly_one_irq() {
    let mut machine = timer_demo().unwrap();
    let mut entries = 0;
    for step in 1..=TIMER_DEMO_STEPS {
        let kind = machine.step().unwrap();
        assert!(machine.last_timing().total() > 0);
        if kind == StepKind::IrqEntry {
            entries += 1;
            assert_eq!(step, 13); // Nominal timing model, not a hardware measurement
            assert_eq!(machine.cpu().pc(), 0x18);
            assert_eq!(machine.cpu().spsr(), Some(0x1f));
            assert_eq!(machine.cpu().registers()[14], ROM_START + 0x30);
            assert_eq!(machine.memory().read16(IF).unwrap(), 8);
        }
    }
    assert_eq!(entries, 1);
    assert_eq!(machine.cycles(), 151); // Initial non-refill ROM fetch is S+S, not N+S.
    assert_eq!(machine.cpu().mode(), Mode::System);
    assert_eq!(machine.cpu().pc(), ROM_START + 0x2c);
    assert_eq!(machine.cpu().registers()[10], 1);
    assert_eq!(machine.memory().read16(IF).unwrap(), 0);
    assert_eq!(machine.memory().read16(TIMER_BASE + 2).unwrap(), 0);
    // IRQ entry gains five cycles before the timer stops; return loses five afterward.
    assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), 0xfff9);
    for _ in 0..100 {
        assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    }
    assert_eq!(machine.cpu().registers()[10], 1);
}

#[test]
fn overflow_after_instruction_is_sampled_on_next_step() {
    let mut bus = Memory::new(words(&[0xe280_0001, 0xe280_0001])).unwrap();
    timer(&mut bus);
    let mut machine = Machine::new(Cpu::new(ROM_START), bus);
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    assert_eq!(machine.cpu().registers()[0], 1);
    assert_eq!(machine.cpu().pc(), ROM_START + 4);
    assert_eq!(machine.memory().read16(IF).unwrap(), 8);
    assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
    assert_eq!(machine.cpu().registers()[0], 1); // Interrupted instruction did not execute
    assert_eq!(machine.cpu().registers()[14], ROM_START + 8);
    assert_eq!(machine.cycles(), 14); // Instruction fetch 6; IRQ source fetch 6 + BIOS pair 2.
}

#[test]
fn missing_vector_errors_only_after_completed_irq_entry() {
    let mut bus = Memory::new(vec![]).unwrap();
    pending_timer(&mut bus);
    let mut machine = Machine::new(Cpu::new(ROM_START), bus);
    assert_eq!(machine.step().unwrap(), StepKind::IrqEntry); // Missing discarded ROM fetch cannot prevent IRQ entry
    let before = machine.cpu().clone();
    let cycles = machine.cycles();
    assert_eq!(
        machine.step(),
        Err(MachineError::Cpu(CpuError::Memory(MemoryError::Unmapped(
            0x18
        ))))
    );
    assert_eq!(machine.cpu(), &before);
    assert_eq!(machine.cycles(), cycles);
    assert_eq!(machine.memory().read16(IF).unwrap(), 8);
}

#[test]
fn cpu_mask_defers_irq_until_msr_unmasks_it() {
    let mut bios = vec![0; BIOS_SIZE];
    bios[0..4].copy_from_slice(&0xe321_f013_u32.to_le_bytes()); // MSR CPSR_c, #SVC (clear I)
    let mut bus = Memory::with_bios(vec![], bios).unwrap();
    pending_timer(&mut bus);
    let mut machine = Machine::new(Cpu::at_reset(), bus);
    assert!(machine.memory().irq_pending());
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    assert_eq!(machine.cpu().pc(), 4);
    assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
    assert_eq!(machine.cpu().registers()[14], 8);
    assert_eq!(machine.cpu().spsr(), Some(0x13));
}

#[test]
fn ime_and_ie_gate_pending_device_requests_until_cpu_enables_them() {
    for register in [IE, IME] {
        let value = if register == IE { 8 } else { 1 };
        let mut machine = prepared(0xe580_1000, &[(0, register), (1, value)]); // STR r1, [r0]
        pending_timer(machine.memory_mut());
        machine.memory_mut().write16(register, 0).unwrap();
        assert!(!machine.memory().irq_pending());
        assert_eq!(machine.step().unwrap(), StepKind::Instruction);
        assert!(machine.memory().irq_pending());
        assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
    }
}

#[test]
fn uncleared_request_reenters_after_irq_return_and_no_fiq_is_generated() {
    let bios = 0xe25e_f004_u32.to_le_bytes().repeat(BIOS_SIZE / 4); // SUBS pc, lr, #4
    let mut bus = Memory::with_bios(words(&[0xeaff_fffe]), bios).unwrap();
    pending_timer(&mut bus);
    let mut machine = Machine::new(Cpu::new(ROM_START), bus);
    for _ in 0..3 {
        assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
        assert_eq!(machine.cpu().mode(), Mode::Irq);
        assert_eq!(machine.step().unwrap(), StepKind::Instruction); // I mask prevents immediate nesting
        assert_eq!(machine.cpu().mode(), Mode::System);
        assert_eq!(machine.cpu().pc(), ROM_START);
    }
    assert_eq!(machine.memory().read16(IF).unwrap(), 8);
}

#[test]
fn timer_irq_returns_to_thumb_without_skipping_the_interrupted_instruction() {
    let mut rom = words(&[0xe28f_0001, 0xe12f_ff10]); // ADD r0, pc, #1; BX r0
    rom.extend(0x2107_u16.to_le_bytes()); // MOV r1, #7 at offset 8
    let bios = 0xe25e_f004_u32.to_le_bytes().repeat(BIOS_SIZE / 4);
    let mut bus = Memory::with_bios(rom, bios).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    cpu.step(&mut bus).unwrap();
    cpu.step(&mut bus).unwrap();
    pending_timer(&mut bus);
    let mut machine = Machine::new(cpu, bus);
    assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
    assert_eq!(machine.cpu().spsr(), Some(0x3f));
    assert_eq!(machine.cpu().registers()[14], ROM_START + 12);
    machine.memory_mut().write16(IF, 8).unwrap();
    machine.step().unwrap();
    assert_eq!(machine.cpu().instruction_set(), InstructionSet::Thumb);
    assert_eq!(machine.cpu().pc(), ROM_START + 8);
    machine.step().unwrap();
    assert_eq!(machine.cpu().registers()[1], 7);
    assert_eq!(machine.cpu().pc(), ROM_START + 10);
}

#[test]
fn diagnostics_do_not_advance_timers_or_clock() {
    for instruction in [0xf3a0_0001, 0xe590_1000] {
        // Unsupported or unmapped LDR
        let mut bus = Memory::new(words(&[instruction])).unwrap();
        timer(&mut bus);
        let mut machine = Machine::new(Cpu::new(ROM_START), bus);
        let before = machine.cpu().clone();
        assert!(machine.step().is_err());
        assert_eq!(machine.cpu(), &before);
        assert_eq!(machine.cycles(), 0);
        assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), 0xffff);
        assert_eq!(machine.memory().read16(IF).unwrap(), 0);
    }
}

#[test]
fn late_block_store_error_does_not_change_timer_reload_or_enable() {
    let mut machine = prepared(
        0xe8a0_003e,
        &[
            (0, TIMER_BASE),
            (1, 0x00c0_ffff),
            (2, 0x00c0_ffff),
            (3, 0x00c0_ffff),
            (4, 0x00c0_ffff),
            (5, 123),
        ],
    ); // STMIA r0!, {r1-r5}; fifth word is unmapped
    let before = machine.cpu().clone();
    assert!(machine.step().is_err());
    assert_eq!(machine.cpu(), &before);
    assert_eq!(machine.cycles(), 0);
    for index in 0..4 {
        let address = TIMER_BASE + index * 4;
        assert_eq!(machine.memory().read32(address).unwrap(), 0);
        machine.memory_mut().write16(address + 2, 0x80).unwrap();
        assert_eq!(machine.memory().read16(address).unwrap(), 0); // Hidden reload also unchanged
    }
}

#[test]
fn late_block_store_error_does_not_acknowledge_if_or_modify_ie() {
    let mut machine = prepared(
        0xe8a0_001e,
        &[(0, IE), (1, 0x0008_ffff), (2, 123), (3, 1), (4, 42)],
    );
    // IE/IF, WAITCNT, IME are valid. The fourth word at 0x0400020c is not.
    pending_timer(machine.memory_mut());
    machine.memory_mut().write32(IME, 0).unwrap(); // Let the invalid store execute
    let before = machine.cpu().clone();
    let cycles = machine.cycles();
    assert!(machine.step().is_err());
    assert_eq!(machine.cpu(), &before);
    assert_eq!(machine.cycles(), cycles);
    assert_eq!(machine.memory().read32(IE).unwrap(), 0x0008_0008);
    assert_eq!(machine.memory().read32(WAITCNT).unwrap(), 0);
    assert_eq!(machine.memory().read32(IME).unwrap(), 0);
    assert_eq!(machine.last_timing().total(), 0);
}

#[test]
fn late_block_load_error_preserves_cpu_and_devices() {
    let mut machine = prepared(0xe8b0_0006, &[(0, IME)]); // LDMIA r0!, {r1, r2}
    timer(machine.memory_mut());
    let before = machine.cpu().clone();
    assert!(machine.step().is_err());
    assert_eq!(machine.cpu(), &before);
    assert_eq!(machine.cycles(), 0);
    assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), 0xffff);
}

#[test]
fn skipped_condition_charges_code_access_but_not_registers() {
    let mut bus = Memory::new(words(&[0x0280_0001])).unwrap(); // ADDEQ r0, r0, #1; Z clear
    timer(&mut bus);
    let mut machine = Machine::new(Cpu::new(ROM_START), bus);
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    assert_eq!(machine.cpu().registers()[0], 0);
    assert_eq!(machine.cpu().pc(), ROM_START + 4);
    assert_eq!(machine.cycles(), 6);
    assert_eq!(machine.memory().read16(IF).unwrap(), 8);
}

#[test]
fn cpu_only_step_does_not_advance_device_time_or_deliver_irq() {
    let mut bus = Memory::new(words(&[0xe280_0001])).unwrap();
    pending_timer(&mut bus);
    let mut cpu = Cpu::new(ROM_START);
    let cycles = bus.cycles();
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers()[0], 1);
    assert_eq!(cpu.mode(), Mode::System);
    assert_eq!(bus.cycles(), cycles);
    assert!(bus.irq_pending());
}
