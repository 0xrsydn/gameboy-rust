//! Original ARM SoftReset. This is a restart service, not a hardware reset.

use super::{ArmImage, IRQ_STACK, ROM_START, SVC_STACK, SYSTEM_STACK};

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
}
