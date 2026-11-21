//! The skipped boot (`bios::skip_boot`): the state Mesen 2's `SkipBootScreen` sets.
use super::*;
use gba_core::{
    audio::SOUNDBIAS,
    bios::{SKIP_BOOT_LINE, SKIP_BOOT_OPEN_BUS},
    io::{BG2PA, BG2PD, BG3PA, BG3PD, DISPCNT, RCNT, VCOUNT},
};

fn skipped(code: &[u32]) -> Machine {
    bios::skip_boot(words(code), bios::image()).unwrap()
}

#[test]
fn skipped_boot_starts_at_the_cartridge_in_system_mode_with_the_firmware_stacks() {
    let mut machine = skipped(&[
        0xe321_f0d2, // MSR CPSR_c,#0xd2 (IRQ)
        0xe1a0_000d, // MOV r0,sp
        0xe321_f0d3, // MSR CPSR_c,#0xd3 (Supervisor)
        0xe1a0_100d, // MOV r1,sp
        0xe321_f01f, // MSR CPSR_c,#0x1f (System)
        0xeaff_fffe, // B .
    ]);
    let cpu = machine.cpu();
    assert_eq!(cpu.pc(), ROM_START);
    assert_eq!(cpu.mode(), Mode::System);
    assert_eq!(cpu.cpsr(), 0x1f); // ARM, IRQ and FIQ unmasked, flags clear.
    assert_eq!(cpu.registers()[13], SYSTEM_STACK);
    assert_eq!(cpu.registers()[14], ROM_START);
    assert!(cpu.registers()[..13].iter().all(|&value| value == 0));
    reach(&mut machine, ROM_START + 20, 10);
    assert_eq!(machine.cpu().registers()[0], bios::IRQ_STACK);
    assert_eq!(machine.cpu().registers()[1], bios::SVC_STACK);
}

#[test]
fn skipped_boot_leaves_the_registers_and_display_line_the_firmware_would() {
    let machine = skipped(&[0xeaff_fffe]);
    let memory = machine.memory();
    for (address, value) in [
        (DISPCNT, 0x0080),
        (SOUNDBIAS, 0x0200),
        (VCOUNT, SKIP_BOOT_LINE as u16),
    ] {
        assert_eq!(memory.read16(address).unwrap(), value, "{address:#x}");
    }
    // General-purpose mode; the low four bits read the idle link-port lines.
    assert_eq!(memory.read16(RCNT).unwrap() & 0xfff0, 0x8000);
    assert_eq!(memory.read8(POSTFLG).unwrap(), 1);
    assert_eq!(memory.display_position().line_cycle, 0);
    // The affine parameters are write-only; the identity shows in BG2/BG3 rendering, which
    // the boot-video tests cover for the firmware. Here the registers took the writes.
    for address in [BG2PA, BG2PD, BG3PA, BG3PD] {
        assert!(memory.read16(address).is_ok(), "{address:#x}");
    }
}

#[test]
fn skipped_boot_latches_the_bios_open_bus_for_protected_reads() {
    let mut machine = skipped(&[
        0xe3a0_0000, // MOV r0,#0
        0xe590_1000, // LDR r1,[r0] (protected: the PC is outside the BIOS)
        0xeaff_fffe, // B .
    ]);
    reach(&mut machine, ROM_START + 8, 10);
    assert_eq!(machine.cpu().registers()[1], SKIP_BOOT_OPEN_BUS);
}

#[test]
fn skipped_boot_keeps_the_supplied_firmware_mapped_for_swis() {
    // SWI 0x06 (Div): 7 / 2 in this emulator's firmware.
    let mut machine = skipped(&[
        0xe3a0_0007, // MOV r0,#7
        0xe3a0_1002, // MOV r1,#2
        0xef06_0000, // SWI 0x06
        0xeaff_fffe, // B .
    ]);
    reach(&mut machine, ROM_START + 12, 10_000);
    assert_eq!(machine.cpu().registers()[0], 3);
    assert_eq!(machine.cpu().registers()[1], 1);
}
