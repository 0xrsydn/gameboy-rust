//! Original run-length decoder emitted as ARM instructions.
//! Runs must fit the declared output size. Halfword output requires an even
//! destination and size; malformed-input diagnostics do not emulate BIOS quirks.

use super::ArmImage;

pub(super) fn emit(a: &mut ArmImage) {
    a.label("rl_wram");
    a.emit(0xe3a0_c000); // MOV r12,#0 (byte stores)
    a.branch(14, "rl_start");
    a.label("rl_vram");
    a.emit(0xe3a0_c001); // MOV r12,#1 (buffered halfword stores)
    a.label("rl_start");
    a.emit(0xe92d_0ff0); // STMDB sp!,{r4-r11} (60 bytes total SVC stack use)
    a.emit(0xe310_0003); // TST r0,#3 (word-aligned header)
    a.branch(1, "invalid_argument");
    a.emit(0xe350_0402); // CMP r0,#0x02000000 (reject protected source region)
    a.branch(3, "invalid_argument");
    a.emit(0xe490_2004); // LDR r2,[r0],#4
    a.emit(0xe202_b0ff); // AND r11,r2,#0xff
    a.emit(0xe35b_0030); // CMP r11,#0x30 (run-length type, reserved bits zero)
    a.branch(1, "invalid_argument");
    a.emit(0xe1a0_2422); // MOV r2,r2,LSR #8 (24-bit output byte count)
    a.emit(0xe35c_0000); // CMP r12,#0
    a.branch(0, "rl_validate_end");
    a.emit(0xe181_b002); // ORR r11,r1,r2
    a.emit(0xe31b_0001); // TST r11,#1 (even destination and length)
    a.branch(1, "invalid_argument");
    a.label("rl_validate_end");
    a.emit(0xe091_b002); // ADDS r11,r1,r2
    a.branch(2, "invalid_argument"); // Reject destination address wrap.
    a.emit(0xe352_0000); // CMP r2,#0
    a.branch(0, "rl_done");
    a.emit(0xe3a0_9000); // MOV r9,#0 (pending halfword)
    a.emit(0xe3a0_a000); // MOV r10,#0 (bit position 0 or 8)

    a.label("rl_block");
    a.emit(0xe4d0_3001); // LDRB r3,[r0],#1 (block control)
    a.emit(0xe203_707f); // AND r7,r3,#0x7f
    a.emit(0xe313_0080); // TST r3,#0x80 (repeat flag)
    a.emit(0x0287_7001); // ADDEQ r7,r7,#1 (1..128 literal bytes)
    a.emit(0x1287_7003); // ADDNE r7,r7,#3 (3..130 repeated bytes)
    a.emit(0xe157_0002); // CMP r7,r2
    a.branch(8, "invalid_argument"); // Reject a block exceeding remaining output.
    a.emit(0xe313_0080); // TST r3,#0x80
    a.branch(0, "rl_literal");
    a.emit(0xe4d0_5001); // LDRB r5,[r0],#1 (read repeated value only once)
    a.label("rl_repeat");
    a.call("decompress_store_byte");
    a.emit(0xe257_7001); // SUBS r7,r7,#1
    a.branch(1, "rl_repeat");
    a.branch(14, "rl_next");
    a.label("rl_literal");
    a.emit(0xe4d0_5001); // LDRB r5,[r0],#1
    a.call("decompress_store_byte");
    a.emit(0xe257_7001); // SUBS r7,r7,#1
    a.branch(1, "rl_literal");
    a.label("rl_next");
    a.emit(0xe352_0000); // CMP r2,#0
    a.branch(1, "rl_block");
    a.label("rl_done");
    a.emit(0xe8bd_0ff0); // LDMIA sp!,{r4-r11}
    a.branch(14, "return");
}
