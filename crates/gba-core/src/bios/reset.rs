//! Original ARM SoftReset and selective RegisterRamReset services.
//! Serial/sound reset requests are diagnostics until those devices are modeled.

use super::{ArmImage, IRQ_STACK, ROM_START, SVC_STACK, SYSTEM_STACK};
use crate::{
    dma::DMA_BASE,
    io::{DISPCNT, IE, IME, TIMER_BASE},
};

pub(super) fn emit(a: &mut ArmImage) {
    a.label("soft_reset");
    // Read the byte before clearing its containing RAM and the common SWI frame.
    // No stack accesses follow: this service never returns to its caller.
    a.literal(0, 0x0300_7ffa);
    a.emit(0xe5d0_2000); // LDRB r2,[r0] (zero: ROM, nonzero: EWRAM)
    a.literal(0, 0x0300_7e00);
    a.emit(0xe3a0_1080); // MOV r1,#128 (512 bytes)
    a.emit(0xe3a0_3000); // MOV r3,#0
    a.label("soft_reset_clear");
    a.emit(0xe480_3004); // STR r3,[r0],#4
    a.emit(0xe251_1001); // SUBS r1,r1,#1
    a.branch(1, "soft_reset_clear");

    a.emit(0xe169_f003); // MSR SPSR_fc,r3 (SVC saved status = 0)
    a.emit(0xe1a0_e003); // MOV lr,r3 (SVC link = 0)
    a.literal(13, SVC_STACK);
    a.emit(0xe321_f0d2); // MSR CPSR_c,#IRQ|I|F
    a.emit(0xe169_f003); // MSR SPSR_fc,r3 (IRQ saved status = 0)
    a.emit(0xe1a0_e003); // MOV lr,r3 (IRQ link = 0)
    a.literal(13, IRQ_STACK);
    a.emit(0xe321_f0df); // MSR CPSR_c,#System|I|F
    a.literal(13, SYSTEM_STACK);
    a.emit(0xe352_0000); // CMP r2,#0
    a.literal(14, ROM_START);
    a.emit(0x13a0_e402); // MOVNE lr,#0x02000000
    for register in 0..=12 {
        a.emit(0xe3a0_0000 | register << 12); // MOV rN,#0
    }
    // Deterministic subset policy: clear NZCV and F, keep I set. Do not change
    // IME, IE, IF, devices, or unrelated CPU banks. IRQ stays masked throughout.
    a.emit(0xe329_f09f); // MSR CPSR_fc,#System|I
    a.emit(0xe12f_ff1e); // BX lr (ARM target, no return through erased SWI frame)
    a.prefetch_tail(0xe129_f000); // Same protected-read word as cold boot.

    emit_ram_reset(a);
}

fn emit_ram_reset(a: &mut ArmImage) {
    a.label("register_ram_reset");
    // Validate unsupported devices before forced blank or any reset writes.
    // Higher flag bits are ignored, as in the documented low-byte flag format.
    a.emit(0xe310_0060); // TST r0,#serial|sound
    a.branch(1, "invalid_argument");
    a.literal(1, DISPCNT);
    a.emit(0xe3a0_2080); // MOV r2,#forced blank (also clear other DISPCNT bits)
    a.emit(0xe1c1_20b0); // STRH r2,[r1]
    a.emit(0xe3a0_3000); // MOV r3,#0 (shared fill value)
    for (flag, start, bytes, next) in [
        (1, 0x0200_0000, 0x40000, "ram_reset_iwram"),
        (2, 0x0300_0000, 0x7e00, "ram_reset_palette"),
        (4, 0x0500_0000, 0x400, "ram_reset_vram"),
        (8, 0x0600_0000, 0x18000, "ram_reset_oam"),
        (16, 0x0700_0000, 0x400, "ram_reset_io"),
    ] {
        a.emit(0xe310_0000 | flag); // TST r0,#flag
        a.branch(0, next);
        clear_words(a, start, bytes);
        a.label(next);
    }
    a.emit(0xe310_0080); // TST r0,#other registers
    a.branch(0, "return");

    // IRQ remains CPU-masked throughout. Disable DMA/timers before clearing
    // their address/count/reload registers. These are bus writes, not host resets.
    a.literal(1, IME);
    store_halfword(a, 0); // IME = 0
    a.literal(1, DMA_BASE);
    for offset in [10, 22, 34, 46] {
        store_halfword(a, offset); // Disable each channel before clearing its registers.
    }
    clear_words(a, DMA_BASE, 48);
    a.literal(1, TIMER_BASE);
    for offset in [2, 6, 10, 14] {
        store_halfword(a, offset); // Stop each timer; the stopped counter remains latched.
    }
    clear_words(a, TIMER_BASE, 16); // Clear reload/control, not hidden counter/clock state.
    clear_words(a, DISPCNT + 4, 0x54); // Display controls, offsets, origins, windows/effects.
    a.literal(1, DISPCNT);
    a.emit(0xe3a0_3c01); // MOV r3,#256 (identity affine PA/PD for BG2 and BG3)
    for offset in [0x20, 0x26, 0x30, 0x36] {
        store_halfword(a, offset);
    }
    a.emit(0xe3a0_3000); // MOV r3,#0
    a.literal(1, IE);
    store_halfword(a, 0); // IE = 0
    store_halfword(a, 4); // WAITCNT = 0
    a.literal(3, 0xffff);
    store_halfword(a, 2); // Acknowledge IF after stopping timer/display IRQ sources.
    a.branch(14, "return");

    // Leaf routine; only r1/r2 change. The common 28-byte SWI frame saves lr.
    a.label("ram_reset_words");
    a.emit(0xe481_3004); // STR r3,[r1],#4
    a.emit(0xe252_2004); // SUBS r2,r2,#4
    a.branch(1, "ram_reset_words");
    a.emit(0xe12f_ff1e); // BX lr
}

fn clear_words(a: &mut ArmImage, address: u32, bytes: u32) {
    debug_assert!(bytes != 0 && bytes % 4 == 0);
    a.literal(1, address);
    a.literal(2, bytes);
    a.call("ram_reset_words");
}

// Store r3 through r1 with an ARM immediate halfword offset.
fn store_halfword(a: &mut ArmImage, offset: u32) {
    debug_assert!(offset <= 255 && offset % 2 == 0);
    a.emit(0xe1c1_30b0 | (offset & 0xf0) << 4 | (offset & 15));
}
