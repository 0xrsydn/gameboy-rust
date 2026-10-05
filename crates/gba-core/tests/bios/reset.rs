use super::*;
use gba_core::{
    dma::DMA_BASE,
    io::{BG0CNT, DISPCNT, WAITCNT},
    memory::{OAM_START, PALETTE_START},
};

const FLAG: u32 = 0x0300_7ffa;
const CLEAR_START: u32 = 0x0300_7e00;
const CLEAR_END: u32 = 0x0300_8000;

fn restart(m: &mut Machine, flag: u8) {
    m.memory_mut().write8(FLAG, flag).unwrap();
    m.step().unwrap(); // Enter the real SWI vector.
    reach(m, if flag == 0 { ROM_START } else { SOURCE }, 2000);
}

fn assert_restart_state(m: &Machine, target: u32) {
    assert_eq!(m.cpu().pc(), target);
    assert_eq!(m.cpu().mode(), Mode::System);
    assert_eq!(m.cpu().instruction_set(), InstructionSet::Arm);
    assert_eq!(m.cpu().cpsr(), 0x9f); // Deterministic policy: IRQ masked, FIQ clear, NZCV clear.
    assert_eq!(&m.cpu().registers()[..13], &[0; 13]);
    assert_eq!(m.cpu().registers()[13], SYSTEM_STACK);
    assert_eq!(m.cpu().registers()[14], target);
}

fn assert_cleared(m: &Machine) {
    for address in CLEAR_START..CLEAR_END {
        assert_eq!(m.memory().read8(address).unwrap(), 0, "{address:#x}");
    }
}

// Inline literal with a branch over its data. No host register mutation.
fn literal(code: &mut Vec<u32>, register: u32, value: u32) {
    code.extend([0xe59f_0000 | register << 12, 0xea00_0000, value]);
}

fn dirty_sp(mode: u32) -> u32 {
    0x0300_7000 + mode * 32
}

fn dirty_lr(mode: u32) -> u32 {
    0xa100_0000 | mode
}

fn seeded_call(thumb: bool, status: u32, location: Option<u32>) -> Machine {
    let mut code = Vec::new();
    for mode in [0x11, 0x12, 0x13, 0x17, 0x1b] {
        code.push(0xe321_f0c0 | mode); // Switch bank with IRQ/FIQ masked.
        literal(&mut code, 13, dirty_sp(mode));
        literal(&mut code, 14, dirty_lr(mode));
        literal(&mut code, 0, 0xb000_00df);
        code.push(0xe169_f000); // MSR SPSR_fc,r0
        if mode == 0x11 {
            for r in 8..=12 {
                code.push(0xe3a0_00e0 | r << 12 | r); // Distinct FIQ high registers.
            }
        }
    }
    code.push(0xe321_f0df);
    literal(&mut code, 13, 0x0300_7dc0);
    for r in 0..=12 {
        code.push(0xe3a0_0080 | r << 12 | r);
    }
    code.extend([0xe328_f20f, 0xe321_f000 | status]); // NZCV set; caller mode/masks.
    let swi_pc = if let Some(address) = location {
        literal(&mut code, 14, address | u32::from(thumb));
        code.push(0xe12f_ff1e);
        address
    } else {
        code.extend([
            if thumb { 0xe28f_e001 } else { 0xe1a0_e00f },
            if thumb { 0xe12f_ff1e } else { 0xe1a0_0000 },
        ]);
        let address = ROM_START + code.len() as u32 * 4;
        code.extend([if thumb { 0xe7fe_df00 } else { 0xef00_0000 }, 0xeaff_fffe]);
        address
    };
    let mut m = bios::boot(words(&code)).unwrap();
    if let Some(address) = location {
        m.memory_mut()
            .write32(address, if thumb { 0xe7fe_df00 } else { 0xef00_0000 })
            .unwrap();
        m.memory_mut().write32(address + 4, 0xeaff_fffe).unwrap();
    }
    reach(&mut m, swi_pc, 500);
    m
}

#[test]
fn every_flag_byte_selects_rom_or_ram_from_arm_and_thumb() {
    for thumb in [false, true] {
        for flag in 0..=255 {
            let (mut m, _) = call(0, thumb, [0x123, 0x456, 0x789]);
            // Only the byte at 7ffa selects the target, not surrounding bytes.
            m.memory_mut().write32(IRQ_FLAGS, 0xffff_ffff).unwrap();
            restart(&mut m, flag);
            assert_restart_state(&m, if flag == 0 { ROM_START } else { SOURCE });
            assert_cleared(&m);
        }
    }
}

#[test]
fn clears_exactly_top_512_bytes_and_keeps_other_memory() {
    for thumb in [false, true] {
        let (mut m, _) = call(0, thumb, [0, 0, 0]);
        for address in (0x0300_0000..CLEAR_END).step_by(4) {
            m.memory_mut()
                .write32(address, address ^ 0xa55a_c33c)
                .unwrap();
        }
        let sentinels = [
            SOURCE,
            SOURCE + 0x3fffc,
            PALETTE_START,
            PALETTE_START + 0x3fc,
            VRAM_START,
            VRAM_START + 0x17ffc,
            OAM_START,
            OAM_START + 0x3fc,
        ];
        for address in sentinels {
            m.memory_mut().write32(address, 0x1357_2468).unwrap();
        }
        let rom_first = m.memory().read32(ROM_START).unwrap();
        restart(&mut m, 0);
        assert_cleared(&m);
        for address in (0x0300_0000..CLEAR_START).step_by(4) {
            assert_eq!(m.memory().read32(address).unwrap(), address ^ 0xa55a_c33c);
        }
        for address in sentinels {
            assert_eq!(m.memory().read32(address).unwrap(), 0x1357_2468);
        }
        assert_eq!(m.memory().read32(ROM_START).unwrap(), rom_first);
    }
}

#[test]
fn resets_nonzero_registers_and_status_for_user_system_callers() {
    for thumb in [false, true] {
        for status in [0x10, 0x50, 0x90, 0xd0, 0x1f, 0x5f, 0x9f, 0xdf] {
            for flag in [0, 1] {
                let mut m = seeded_call(thumb, status, None);
                assert!(m.cpu().registers()[..13].iter().all(|r| *r != 0));
                assert_eq!(m.cpu().cpsr() & 0xf000_0000, 0xf000_0000);
                restart(&mut m, flag);
                assert_restart_state(&m, if flag == 0 { ROM_START } else { SOURCE });
                assert_cleared(&m);
            }
        }
    }
}

#[test]
fn reinitializes_supervisor_irq_stacks_links_and_saved_status() {
    for thumb in [false, true] {
        let mut m = seeded_call(thumb, 0x1f, None);
        for (i, mode) in [0xd3, 0xd2, 0xdf].into_iter().enumerate() {
            m.memory_mut()
                .write32(SOURCE + i as u32 * 4, 0xe321_f000 | mode)
                .unwrap();
        }
        restart(&mut m, 1);
        for (mode, stack) in [
            (Mode::Supervisor, bios::SVC_STACK),
            (Mode::Irq, bios::IRQ_STACK),
        ] {
            m.step().unwrap();
            assert_eq!(m.cpu().mode(), mode);
            assert_eq!(m.cpu().registers()[13], stack);
            assert_eq!(m.cpu().registers()[14], 0);
            assert_eq!(m.cpu().spsr(), Some(0));
        }
        m.step().unwrap();
        assert_eq!(m.cpu().mode(), Mode::System);
        assert_eq!(m.cpu().registers()[13], SYSTEM_STACK);
        assert_eq!(m.cpu().registers()[14], SOURCE);
        assert_cleared(&m);
    }
}

#[test]
fn unrelated_banked_registers_and_saved_status_are_preserved() {
    let mut m = seeded_call(false, 0x1f, None);
    for (i, mode) in [0x11, 0x17, 0x1b].into_iter().enumerate() {
        m.memory_mut()
            .write32(SOURCE + i as u32 * 4, 0xe321_f0c0 | mode)
            .unwrap();
    }
    restart(&mut m, 1);
    for mode in [Mode::Fiq, Mode::Abort, Mode::Undefined] {
        m.step().unwrap();
        assert_eq!(m.cpu().mode(), mode);
        assert_eq!(m.cpu().registers()[13], dirty_sp(mode as u32));
        assert_eq!(m.cpu().registers()[14], dirty_lr(mode as u32));
        assert_eq!(m.cpu().spsr(), Some(0xb000_00df));
        if mode == Mode::Fiq {
            assert_eq!(&m.cpu().registers()[8..13], &[0xe8, 0xe9, 0xea, 0xeb, 0xec]);
        } else {
            assert_eq!(&m.cpu().registers()[8..13], &[0; 5]);
        }
    }
}

#[test]
fn can_restart_from_work_ram_including_code_in_erased_area() {
    for thumb in [false, true] {
        for location in [SOURCE + 0x100, 0x0300_1000, CLEAR_START, 0x03ff_fe00] {
            let mut m = seeded_call(thumb, 0x1f, Some(location));
            restart(&mut m, 1);
            assert_restart_state(&m, SOURCE);
            assert_cleared(&m);
            if location < CLEAR_START {
                assert_eq!(
                    m.memory().read32(location).unwrap(),
                    if thumb { 0xe7fe_df00 } else { 0xef00_0000 }
                );
            }
        }
    }
}

#[test]
fn ram_restart_executes_arm_code_and_can_call_bios_again() {
    for thumb in [false, true] {
        let mut m = seeded_call(thumb, 0x10, None);
        // This program is only valid as ARM: Sqrt(81), then a self-branch.
        for (i, instruction) in [0xe3a0_0051, 0xef08_0000, 0xeaff_fffe]
            .into_iter()
            .enumerate()
        {
            m.memory_mut()
                .write32(SOURCE + i as u32 * 4, instruction)
                .unwrap();
        }
        restart(&mut m, 1);
        reach(&mut m, SOURCE + 8, 1000);
        assert_eq!(m.cpu().registers()[0], 9);
        assert_eq!(m.cpu().registers()[13], SYSTEM_STACK);
        assert_eq!(m.cpu().registers()[14], SOURCE);
        assert_eq!(m.cpu().cpsr(), 0x9f);
    }
}

#[test]
fn does_not_reset_io_or_device_clocks() {
    for thumb in [false, true] {
        let (mut m, _) = call(0, thumb, [0, 0, 0]);
        for (address, value) in [
            (DISPCNT, 0x0403),
            (BG0CNT, 0x0141),
            (WAITCNT, 0x4317),
            (DISPSTAT, 0x7b38),
            (IE, 0x1234),
            (IME, 1),
        ] {
            m.memory_mut().write16(address, value).unwrap();
        }
        m.memory_mut().write32(TIMER_BASE, 0x0080_0000).unwrap();
        let timer_before = m.memory().read16(TIMER_BASE).unwrap();
        let cycles = m.cycles();
        restart(&mut m, 0);
        for (address, value) in [
            (DISPCNT, 0x0403),
            (BG0CNT, 0x0141),
            (WAITCNT, 0x4317),
            (IE, 0x1234),
            (IME, 1),
            (POSTFLG, 1),
            (TIMER_BASE + 2, 0x80),
        ] {
            assert_eq!(m.memory().read16(address).unwrap(), value, "{address:#x}");
        }
        assert_eq!(m.memory().read16(DISPSTAT).unwrap() & 0xff38, 0x7b38);
        let elapsed = m.cycles() - cycles;
        assert!(elapsed > 512 && elapsed < 65536);
        assert_eq!(
            m.memory().read16(TIMER_BASE).unwrap(),
            timer_before.wrapping_add(elapsed as u16)
        );
        assert!(!m.halted());
    }
}

#[test]
fn dma_timers_and_pending_irq_continue_but_irq_stays_masked_after_restart() {
    for thumb in [false, true] {
        for ime in [0, 1] {
            let (mut m, _) = call(0, thumb, [0, 0, 0]);
            m.memory_mut().write8(FLAG, 1).unwrap();
            m.memory_mut().write32(SOURCE, 0xeaff_fffe).unwrap();
            m.step().unwrap();
            // Queue IRQ and DMA only after SWI entry; otherwise IRQ can win first.
            m.memory_mut().write16(IE, 8).unwrap();
            m.memory_mut().write16(IME, ime).unwrap();
            m.memory_mut().write32(TIMER_BASE, 0x00c0_fff0).unwrap();
            m.memory_mut().write32(DEST, 0x8765_4321).unwrap();
            m.memory_mut().write32(DMA_BASE, DEST).unwrap();
            m.memory_mut().write32(DMA_BASE + 4, DEST + 4).unwrap();
            m.memory_mut().write32(DMA_BASE + 8, 0x8400_0001).unwrap();
            let mut dma = 0;
            for _ in 0..2000 {
                if m.cpu().pc() == SOURCE {
                    break;
                }
                let step = m.step().unwrap();
                assert_ne!(step, StepKind::IrqEntry);
                if matches!(step, StepKind::Dma { channel: 0 }) {
                    dma += 1;
                }
            }
            assert_restart_state(&m, SOURCE);
            assert_eq!(dma, 1);
            assert_eq!(m.memory().read32(DEST + 4).unwrap(), 0x8765_4321);
            assert_eq!(m.memory().read16(IF).unwrap() & 8, 8);
            assert_eq!(m.memory().read16(IME).unwrap(), ime);
            assert_cleared(&m);
            assert_eq!(m.step().unwrap(), StepKind::Instruction);
            assert_eq!(m.cpu().pc(), SOURCE);
        }
    }
}

#[test]
fn reset_is_not_an_atomic_host_memory_clear() {
    let (mut m, _) = call(0, false, [0, 0, 0]);
    for address in (CLEAR_START..CLEAR_END).step_by(4) {
        m.memory_mut().write32(address, 0xa55a_c33c).unwrap();
    }
    m.memory_mut().write8(FLAG, 0).unwrap();
    m.step().unwrap();
    for _ in 0..1000 {
        if m.memory().read32(CLEAR_START).unwrap() == 0 {
            break;
        }
        m.step().unwrap();
    }
    assert_eq!(m.memory().read32(CLEAR_START).unwrap(), 0);
    assert_eq!(m.memory().read32(CLEAR_START + 4).unwrap(), 0xa55a_c33c);
    assert_eq!(m.memory().read32(IRQ_HANDLER).unwrap(), 0xa55a_c33c);
    assert!(m.cpu().pc() < BIOS_SIZE as u32);
    reach(&mut m, ROM_START, 2000);
    assert_cleared(&m);
}

#[test]
fn exception_mode_callers_still_receive_a_diagnostic() {
    for status in [0x12, 0x13] {
        let (mut m, _) = call_status(0, false, [0, 0, 0], status);
        m.memory_mut().write32(CLEAR_START, 0x1357_2468).unwrap();
        let error = (0..100).find_map(|_| m.step().err()).unwrap();
        assert!(matches!(
            error,
            MachineError::Cpu(CpuError::UnsupportedInstruction {
                instruction: UNSUPPORTED_TRAP,
                ..
            })
        ));
        assert_eq!(m.memory().read32(CLEAR_START).unwrap(), 0x1357_2468);
    }
}
