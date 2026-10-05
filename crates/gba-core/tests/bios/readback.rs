//! Firmware boundary outputs, checked through CPU loads rather than host reads.
use super::*;
use gba_core::cpu::Cpu;

const PROBE: u32 = 0x0203_0000;

// A separate CPU leaves the caller's registers and device clocks unchanged.
// All probe instructions run outside BIOS, so they cannot replace its retained word.
fn probe(bus: &mut Memory, thumb: bool, address: u32, arm: u32, half: u16) -> u32 {
    let code = if thumb {
        vec![
            0xe28f_3001,                    // ADD r3,pc,#1
            0xe12f_ff13,                    // BX r3
            u32::from(half) << 16 | 0x4801, // LDR r0,[pc,#4]; load r1
            0xe7fe_e7fe,
            address,
        ]
    } else {
        vec![0xe59f_0004, arm, 0xeaff_fffe, address]
    };
    for (index, word) in code.into_iter().enumerate() {
        bus.write32(PROBE + index as u32 * 4, word).unwrap();
    }
    let mut cpu = Cpu::new(PROBE);
    for _ in 0..if thumb { 4 } else { 2 } {
        cpu.step(bus).unwrap();
    }
    cpu.registers()[1]
}

fn assert_readback(machine: &mut Machine, expected: u32) {
    for thumb in [false, true] {
        for base in [0, 0x3ffc] {
            assert_eq!(
                probe(machine.memory_mut(), thumb, base, 0xe590_1000, 0x6801),
                expected
            );
            for lane in 0..4 {
                let byte = (expected >> (lane * 8)) & 0xff;
                assert_eq!(
                    probe(
                        machine.memory_mut(),
                        thumb,
                        base + lane,
                        0xe5d0_1000,
                        0x7801
                    ),
                    byte
                );
                assert_eq!(
                    probe(
                        machine.memory_mut(),
                        thumb,
                        base + lane,
                        0xe1d0_10d0,
                        0x5681
                    ),
                    byte as u8 as i8 as i32 as u32
                );
            }
            for lane in [0, 2] {
                let half = (expected >> (lane * 8)) & 0xffff;
                assert_eq!(
                    probe(
                        machine.memory_mut(),
                        thumb,
                        base + lane,
                        0xe1d0_10b0,
                        0x8801
                    ),
                    half
                );
                assert_eq!(
                    probe(
                        machine.memory_mut(),
                        thumb,
                        base + lane,
                        0xe1d0_10f0,
                        0x5e81
                    ),
                    half as u16 as i16 as i32 as u32
                );
            }
        }
    }
}

// Return the actual last BIOS instruction, not a fixed firmware address.
fn reach_exit(machine: &mut Machine, target: u32, limit: usize) -> u32 {
    let mut exit = None;
    for _ in 0..limit {
        if machine.cpu().pc() == target {
            return exit.expect("must leave BIOS before target");
        }
        let pc = machine.cpu().pc();
        machine.step().unwrap();
        if pc < BIOS_SIZE as u32 && machine.cpu().pc() >= BIOS_SIZE as u32 {
            exit = Some(pc);
        }
    }
    panic!("did not reach {target:#x}");
}

fn assert_tail(machine: &Machine, exit: u32, expected: u32) {
    assert_eq!(machine.memory().read32(exit + 4).unwrap(), UNSUPPORTED_TRAP);
    assert_eq!(machine.memory().read32(exit + 8).unwrap(), expected);
}

#[test]
fn boot_readback_comes_from_guarded_firmware_tail() {
    let mut machine = bios::boot(words(&[0xeaff_fffe])).unwrap();
    let exit = reach_exit(&mut machine, ROM_START, 100);
    assert_tail(&machine, exit, 0xe129_f000);
    assert_readback(&mut machine, 0xe129_f000);
    assert_eq!(machine.cpu().cpsr(), 0x1f);
    assert_eq!(machine.cpu().registers()[13], SYSTEM_STACK);
}

#[test]
fn soft_reset_readback_matches_both_restart_targets_and_caller_states() {
    for thumb in [false, true] {
        for status in [0x10, 0x1f] {
            for flag in [0, 1] {
                let (mut machine, _) = call_status(0, thumb, [1, 2, 3], status);
                machine.memory_mut().write8(0x0300_7ffa, flag).unwrap();
                machine.step().unwrap();
                let target = if flag == 0 { ROM_START } else { SOURCE };
                let exit = reach_exit(&mut machine, target, 2000);
                assert_tail(&machine, exit, 0xe129_f000);
                assert_readback(&mut machine, 0xe129_f000);
                assert_eq!(machine.cpu().cpsr(), 0x9f);
                assert_eq!(&machine.cpu().registers()[..13], &[0; 13]);
                assert_eq!(machine.cpu().registers()[13], SYSTEM_STACK);
                assert_eq!(machine.cpu().registers()[14], target);
            }
        }
    }
}

#[test]
fn returning_services_share_readback_without_changing_caller_status() {
    for thumb in [false, true] {
        for status in [0x10, 0x1f, 0x9f] {
            for service in [1, 8, 0x0b, 0x0c] {
                let (mut machine, target) = call_status(service, thumb, [0, 0, 0], status);
                let before = machine.cpu().clone();
                machine.step().unwrap();
                let exit = reach_exit(&mut machine, target, 1000);
                assert_tail(&machine, exit, 0xe3a0_2004);
                assert_readback(&mut machine, 0xe3a0_2004);
                assert_eq!(machine.cpu().cpsr(), before.cpsr());
                assert_eq!(&machine.cpu().registers()[..15], &before.registers()[..15]);
            }
        }
    }
}

#[test]
fn irq_callback_and_return_have_distinct_image_derived_readback() {
    for thumb in [false, true] {
        for status in [0x10, 0x1f] {
            let (mut machine, _) = call_status(8, thumb, [25, 2, 3], status);
            let before = machine.cpu().clone();
            callback(machine.memory_mut());
            let prefix = CALLBACK + 0x80;
            for (index, word) in [
                0xe3a0_0000, // MOV r0,#0
                0xe590_1000, // LDR r1,[r0] (protected BIOS read)
                0xe59f_2008, // LDR r2,[pc,#8]
                0xe582_1000, // STR r1,[r2]
                0xe3a0_0301, // MOV r0,#I/O base
                0xeaff_ffd9, // B CALLBACK (keep lr)
                DEST,
            ]
            .into_iter()
            .enumerate()
            {
                machine
                    .memory_mut()
                    .write32(prefix + index as u32 * 4, word)
                    .unwrap();
            }
            machine.memory_mut().write32(IRQ_HANDLER, prefix).unwrap();
            machine.memory_mut().write16(DISPSTAT, 8).unwrap();
            machine.memory_mut().write16(IE, 1).unwrap();
            machine.memory_mut().write16(IME, 1).unwrap();
            machine.memory_mut().advance_cycles(VBLANK_START);
            assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
            let dispatch = reach_exit(&mut machine, prefix, 100);
            assert_eq!(machine.memory().read32(dispatch + 4).unwrap(), 0xe8bd_500f);
            assert_eq!(machine.memory().read32(dispatch + 8).unwrap(), 0xe25e_f004);
            assert_readback(&mut machine, 0xe25e_f004);
            let exit = reach_exit(&mut machine, before.pc(), 100);
            assert_tail(&machine, exit, 0xe55e_c002);
            assert_eq!(machine.memory().read32(DEST).unwrap(), 0xe25e_f004);
            assert_readback(&mut machine, 0xe55e_c002);
            assert_eq!(machine.cpu().cpsr(), before.cpsr());
            assert_eq!(machine.cpu().registers(), before.registers());
            assert_eq!(machine.memory().read16(IF).unwrap(), 0);
            assert_eq!(machine.memory().read16(IRQ_FLAGS).unwrap(), 1);
        }
    }
}

#[test]
fn interrupt_wait_replaces_irq_readback_with_swi_readback_on_return() {
    for thumb in [false, true] {
        let (mut machine, target) = call(5, thumb, [1, 2, 3]);
        let before = machine.cpu().clone();
        callback(machine.memory_mut());
        machine.memory_mut().write16(DISPSTAT, 8).unwrap();
        machine.memory_mut().write16(IE, 1).unwrap();
        machine.step().unwrap();
        let mut slept = false;
        let mut irqs = 0;
        let mut callback_seen = false;
        for _ in 0..1000 {
            if machine.cpu().pc() == target {
                break;
            }
            if machine.cpu().pc() == CALLBACK {
                callback_seen = true;
                assert_readback(&mut machine, 0xe25e_f004);
            }
            match machine.step().unwrap() {
                StepKind::HaltIdle => slept = true,
                StepKind::IrqEntry => irqs += 1,
                _ => {}
            }
        }
        assert!(slept && callback_seen);
        assert_eq!(irqs, 1);
        assert_eq!(machine.cpu().pc(), target);
        assert_readback(&mut machine, 0xe3a0_2004);
        assert_eq!(machine.cpu().cpsr(), before.cpsr());
        assert_eq!(&machine.cpu().registers()[..15], &before.registers()[..15]);
        assert_eq!(machine.memory().read16(IRQ_FLAGS).unwrap(), 0);
    }
}

#[test]
fn supplied_image_tail_is_not_overridden_by_compatibility_constants() {
    let rom = words(&[0xeaff_fffe]);
    let mut original = bios::boot(rom.clone()).unwrap();
    let exit = reach_exit(&mut original, ROM_START, 100);
    let mut image = bios::image();
    let value: u32 = 0x89ab_cdef;
    image[exit as usize + 8..exit as usize + 12].copy_from_slice(&value.to_le_bytes());
    let mut machine = Machine::new(Cpu::at_reset(), Memory::with_bios(rom, image).unwrap());
    assert_eq!(reach_exit(&mut machine, ROM_START, 100), exit);
    assert_readback(&mut machine, value);
    assert_ne!(machine.memory().read32(0).unwrap(), value); // Host reads remain raw.
}
