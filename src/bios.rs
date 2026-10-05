//! Original, optional ARM BIOS replacement. No Nintendo firmware is included.
//! Supports interrupt waits and memory copy/fill.
//! This is a functional subset, not a complete boot ROM or a timing-compatible BIOS.

use std::collections::BTreeMap;


use crate::{
    cpu::Cpu,
    io::{HALTCNT, IME, POSTFLG},
    machine::Machine,
    memory::{Memory, MemoryError, BIOS_SIZE, ROM_START},
};

pub const SYSTEM_STACK: u32 = 0x0300_7f00;
pub const IRQ_STACK: u32 = 0x0300_7fa0;
pub const SVC_STACK: u32 = 0x0300_7fe0;
pub const IRQ_FLAGS: u32 = 0x0300_7ff8;
pub const IRQ_HANDLER: u32 = 0x0300_7ffc;
/// An intentional undefined instruction used for unsupported SWIs/vectors and invalid IRQ pointers.
pub const UNSUPPORTED_TRAP: u32 = 0xe7f0_00f0;
/// A separate diagnostic trap for division by zero or invalid unpacking/decompression arguments.
pub const INVALID_ARGUMENT_TRAP: u32 = 0xe7f0_00f1;

/// Start at reset with our optional firmware. Call Machine::step to execute boot.
/// Boot initializes three stacks, POSTFLG, and the IRQ communication words, then
/// enters ROM_START in ARM System mode. It does not validate a cartridge header,
/// reproduce Nintendo's boot process, or provide file loading.
pub fn boot(rom: Vec<u8>) -> Result<Machine, MemoryError> {
    Ok(Machine::new(
        Cpu::at_reset(),
        Memory::with_bios(rom, image())?,
    ))
}

/// Build a deterministic 16 KiB image from original ARM instructions.
/// SWIs use bits 16–23 of the ARM immediate, or the Thumb immediate byte.
/// Services support User/System callers and preserve caller status. Registers
/// other than each arithmetic service's documented outputs are preserved.
/// IRQ callbacks must preserve r4–r11, acknowledge IF, update IRQ_FLAGS for wait
/// services, and return with BX lr. Nested IRQs and SWIs from callbacks are unsupported.
/// Service routines use the Supervisor stack; exact firmware stack layout and
/// undocumented BIOS side effects are not reproduced.
pub fn image() -> Vec<u8> {
    let mut a = ArmImage::default();
    for vector in [
        "reset",
        "unsupported",
        "swi",
        "unsupported",
        "unsupported",
        "unsupported",
        "irq",
        "unsupported",
    ] {
        a.branch(14, vector);
    }

    a.label("reset");
    a.emit(0xe321_f0d3); // MSR CPSR_c,#SVC|I|F
    a.literal(13, SVC_STACK);
    a.emit(0xe321_f0d2); // MSR CPSR_c,#IRQ|I|F
    a.literal(13, IRQ_STACK);
    a.emit(0xe321_f0df); // MSR CPSR_c,#System|I|F
    a.literal(13, SYSTEM_STACK);
    a.emit(0xe3a0_0000); // MOV r0,#0
    a.literal(1, IRQ_FLAGS);
    a.emit(0xe581_0000); // STR r0,[r1] (flags and reserved upper halfword)
    a.emit(0xe581_0004); // STR r0,[r1,#4] (callback pointer)
    a.literal(1, IME);
    a.emit(0xe581_0000); // STR r0,[r1] (disable IRQ delivery during minimal boot)
    a.literal(1, POSTFLG);
    a.emit(0xe3a0_0001); // MOV r0,#1
    a.emit(0xe5c1_0000); // STRB r0,[r1]
    a.emit(0xe321_f01f); // MSR CPSR_c,#System
    a.literal(15, ROM_START); // LDR pc,=ROM_START

    a.label("irq");
    a.emit(0xe92d_500f); // STMDB sp!,{r0-r3,r12,lr}
    a.literal(1, IRQ_HANDLER);
    a.emit(0xe591_1000); // LDR r1,[r1]
    a.emit(0xe351_0000); // CMP r1,#0
    a.branch(0, "unsupported");
    a.emit(0xe311_0003); // TST r1,#3 (hardware callback is word-aligned ARM)
    a.branch(1, "unsupported");
    a.emit(0xe3a0_0301); // MOV r0,#0x04000000 (callback convention)
    a.emit(0xe1a0_e00f); // MOV lr,pc (return after BX)
    a.emit(0xe12f_ff11); // BX r1
    a.emit(0xe8bd_500f); // LDMIA sp!,{r0-r3,r12,lr}
    a.emit(0xe25e_f004); // SUBS pc,lr,#4

    a.label("swi");
    a.emit(0xe92d_500f); // STMDB sp!,{r0-r3,r12,lr}
    a.emit(0xe14f_3000); // MRS r3,SPSR
    a.emit(0xe92d_0008); // STMDB sp!,{r3} (save caller status across IRQs)
                         // Keep stack usage bounded within the default 64-byte Supervisor area.
                         // In particular, reject a nested SWI from an IRQ callback during a wait.
    a.emit(0xe203_c01f); // AND r12,r3,#mode mask
    a.emit(0xe35c_0010); // CMP r12,#User
    a.branch(0, "decode_swi");
    a.emit(0xe35c_001f); // CMP r12,#System
    a.branch(1, "unsupported");
    a.label("decode_swi");
    a.emit(0xe313_0020); // TST r3,#Thumb
    a.emit(0x115e_c0b2); // LDRHNE r12,[lr,#-2]
    a.emit(0x051e_c004); // LDREQ r12,[lr,#-4]
    a.emit(0x01a0_c82c); // MOVEQ r12,r12,LSR #16
    a.emit(0xe20c_c0ff); // AND r12,r12,#0xff
    for (number, target) in [
        (2, "halt"),
        (4, "intr_wait"),
        (5, "vblank_wait"),
        (0x0b, "cpu_set"),
        (0x0c, "cpu_fast_set"),
    ] {
        a.emit(0xe35c_0000 | number); // CMP r12,#service
        a.branch(0, target);
    }
    a.branch(14, "unsupported");

    a.label("halt");
    a.literal(1, HALTCNT);
    a.emit(0xe3a0_0000); // MOV r0,#0
    a.emit(0xe5c1_0000); // STRB r0,[r1]
    a.branch(14, "return");

    a.label("vblank_wait");
    a.emit(0xe3a0_0001); // MOV r0,#1 (discard old flags)
    a.emit(0xe3a0_1001); // MOV r1,#VBlank
    a.label("intr_wait");
    a.literal(12, IME);
    a.emit(0xe3a0_3001); // MOV r3,#1
    a.emit(0xe58c_3000); // STR r3,[r12] (IME=1)
    a.literal(2, IRQ_FLAGS);
    a.emit(0xe350_0000); // CMP r0,#0
    a.branch(0, "wait_unmask");
    a.emit(0xe1d2_30b0); // LDRH r3,[r2]
    a.emit(0xe1c3_3001); // BIC r3,r3,r1
    a.emit(0xe1c2_30b0); // STRH r3,[r2]
    a.label("wait_unmask");
    a.emit(0xe10f_3000); // MRS r3,CPSR
    a.emit(0xe3c3_3080); // BIC r3,r3,#I
    a.emit(0xe121_f003); // MSR CPSR_c,r3
    a.label("wait_loop");
    // Mask IME while checking flags and entering HALT. A new hardware request
    // still wakes HALT, but cannot be acknowledged between the check and sleep.
    a.emit(0xe3a0_3000); // MOV r3,#0
    a.emit(0xe58c_3000); // STR r3,[r12] (IME=0)
    a.emit(0xe1d2_30b0); // LDRH r3,[r2]
    a.emit(0xe113_0001); // TST r3,r1
    a.branch(1, "wait_done");
    a.literal(0, HALTCNT);
    a.emit(0xe3a0_3000); // MOV r3,#0
    a.emit(0xe5c0_3000); // STRB r3,[r0] (HALT)
    a.emit(0xe3a0_3001); // MOV r3,#1
    a.emit(0xe58c_3000); // STR r3,[r12] (IME=1, allow IRQ callback to record flags)
    a.branch(14, "wait_loop");
    a.label("wait_done");
    a.emit(0xe1c3_3001); // BIC r3,r3,r1 (consume selected flags only)
    a.emit(0xe1c2_30b0); // STRH r3,[r2]
    a.emit(0xe3a0_3001); // MOV r3,#1
    a.emit(0xe58c_3000); // STR r3,[r12]
    a.branch(14, "return");

    a.label("cpu_set");
    emit_count(&mut a);
    a.emit(0xe312_0301); // TST r2,#0x04000000 (word width)
    a.branch(1, "copy_word");
    a.branch(14, "copy_halfword");
    a.label("cpu_fast_set");
    emit_count(&mut a);
    a.emit(0xe283_3007); // ADD r3,r3,#7
    a.emit(0xe3c3_3007); // BIC r3,r3,#7 (round up to eight words)
    emit_copy_checks(&mut a, true);
    a.emit(0xe92d_0ff0); // STMDB sp!,{r4-r11} (60 bytes total SVC stack use)
    a.emit(0xe312_0401); // TST r2,#0x01000000
    a.branch(0, "fast_copy_loop");
    a.emit(0xe590_4000); // LDR r4,[r0] (read fill value once)
    for register in 5..=11 {
        a.emit(0xe1a0_0004 | register << 12);
    } // MOV rN,r4
    a.label("fast_fill_loop");
    a.emit(0xe8a1_0ff0); // STMIA r1!,{r4-r11}
    a.emit(0xe253_3008); // SUBS r3,r3,#8
    a.branch(1, "fast_fill_loop");
    a.branch(14, "fast_done");
    a.label("fast_copy_loop");
    a.emit(0xe8b0_0ff0); // LDMIA r0!,{r4-r11}
    a.emit(0xe8a1_0ff0); // STMIA r1!,{r4-r11}
    a.emit(0xe253_3008); // SUBS r3,r3,#8
    a.branch(1, "fast_copy_loop");
    a.label("fast_done");
    a.emit(0xe8bd_0ff0); // LDMIA sp!,{r4-r11}
    a.branch(14, "return");
    emit_copy(
        &mut a,
        true,
        ["copy_word", "word_loop", "word_fill", "word_fill_loop"],
    );
    emit_copy(
        &mut a,
        false,
        ["copy_halfword", "half_loop", "half_fill", "half_fill_loop"],
    );


    a.label("return");
    a.emit(0xe10f_3000); // MRS r3,CPSR
    a.emit(0xe383_3080); // ORR r3,r3,#I (protect status/stack restoration)
    a.emit(0xe121_f003); // MSR CPSR_c,r3
    a.emit(0xe8bd_0008); // LDMIA sp!,{r3}
    a.emit(0xe169_f003); // MSR SPSR_fc,r3
    a.emit(0xe8bd_500f); // LDMIA sp!,{r0-r3,r12,lr}
    a.emit(0xe1b0_f00e); // MOVS pc,lr
    a.label("unsupported");
    a.emit(UNSUPPORTED_TRAP);
    a.label("invalid_argument");
    a.emit(INVALID_ARGUMENT_TRAP);
    a.finish()
}

fn emit_count(a: &mut ArmImage) {
    a.emit(0xe1a0_3582); // MOV r3,r2,LSL #11
    a.emit(0xe1a0_35a3); // MOV r3,r3,LSR #11 (low 21 bits)
}

fn emit_copy_checks(a: &mut ArmImage, word: bool) {
    a.emit(0xe353_0000); // CMP r3,#0
    a.branch(0, "return");
    a.emit(if word { 0xe3c0_0003 } else { 0xe3c0_0001 }); // BIC r0,r0,#alignment mask
    a.emit(if word { 0xe3c1_1003 } else { 0xe3c1_1001 }); // BIC r1,r1,#alignment mask
                                                          // Reject protected source ranges and wrapping end addresses before writing.
    a.emit(0xe350_0402); // CMP r0,#0x02000000
    a.branch(3, "return"); // BLO
    a.emit(if word { 0xe090_c103 } else { 0xe090_c083 }); // ADDS r12,r0,r3,LSL #2/#1
    a.branch(2, "return"); // BCS (address overflow)
    a.emit(0xe35c_0402); // CMP r12,#0x02000000
    a.branch(3, "return");
}

fn emit_copy(a: &mut ArmImage, word: bool, labels: [&'static str; 4]) {
    let [entry, copy_loop, fill, fill_loop] = labels;
    a.label(entry);
    emit_copy_checks(a, word);
    a.emit(0xe312_0401); // TST r2,#0x01000000 (fixed source)
    a.branch(1, fill);
    a.label(copy_loop);
    a.emit(if word { 0xe490_2004 } else { 0xe0d0_20b2 }); // LDR/LDRH r2,[r0],#width
    a.emit(if word { 0xe481_2004 } else { 0xe0c1_20b2 }); // STR/STRH r2,[r1],#width
    a.emit(0xe253_3001); // SUBS r3,r3,#1
    a.branch(1, copy_loop);
    a.branch(14, "return");
    a.label(fill);
    a.emit(if word { 0xe590_2000 } else { 0xe1d0_20b0 }); // Read fixed source once.
    a.label(fill_loop);
    a.emit(if word { 0xe481_2004 } else { 0xe0c1_20b2 });
    a.emit(0xe253_3001);
    a.branch(1, fill_loop);
    a.branch(14, "return");
}

/// Small image builder for branch/literal fixups. Not a general ARM assembler.
#[derive(Default)]
struct ArmImage {
    code: Vec<u32>,
    labels: BTreeMap<&'static str, usize>,
    branches: Vec<(usize, u32, &'static str, bool)>,
    literals: Vec<(usize, u32, u32)>,
}

impl ArmImage {
    fn emit(&mut self, instruction: u32) {
        self.code.push(instruction);
    }

    fn label(&mut self, name: &'static str) {
        assert!(
            self.labels.insert(name, self.code.len()).is_none(),
            "duplicate BIOS label"
        );
    }

    fn branch(&mut self, condition: u32, target: &'static str) {
        self.branches
            .push((self.code.len(), condition, target, false));
        self.emit(0);
    }

    fn call(&mut self, target: &'static str) {
        self.branches.push((self.code.len(), 14, target, true));
        self.emit(0);
    }

    fn literal(&mut self, register: u32, value: u32) {
        self.literals.push((self.code.len(), register, value));
        self.emit(0);
    }

    fn finish(mut self) -> Vec<u8> {
        for (index, condition, target, link) in self.branches {
            let offset = self.labels[target] as i32 - index as i32 - 2;
            assert!((-0x80_0000..0x80_0000).contains(&offset));
            self.code[index] = condition << 28
                | 0x0a00_0000
                | (u32::from(link) << 24)
                | (offset as u32 & 0x00ff_ffff);
        }
        for (index, register, value) in self.literals {
            let offset = (self.code.len() as i32 - index as i32 - 2) * 4;
            assert!(offset.unsigned_abs() <= 0xfff);
            self.code[index] = (if offset < 0 { 0xe51f_0000 } else { 0xe59f_0000 })
                | register << 12
                | offset.unsigned_abs();
            self.code.push(value);
        }
        let mut bytes: Vec<u8> = self.code.into_iter().flat_map(u32::to_le_bytes).collect();
        assert!(bytes.len() <= BIOS_SIZE);
        bytes.resize(BIOS_SIZE, 0);
        bytes
    }
}
