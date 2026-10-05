//! Original differential filters emitted as ARM instructions.
//! Byte or halfword deltas reconstruct samples through modular addition.
//! Invalid headers/alignment produce diagnostics, not firmware-quirk emulation.

use super::ArmImage;

pub(super) fn emit(a: &mut ArmImage) {
    a.label("diff8_wram");
    a.emit(0xe3a0_c000); // MOV r12,#0 (byte input and output)
    a.branch(14, "diff_start");
    a.label("diff8_vram");
    a.emit(0xe3a0_c001); // MOV r12,#1 (byte input, buffered halfword output)
    a.branch(14, "diff_start");
    a.label("diff16");
    a.emit(0xe3a0_c002); // MOV r12,#2 (halfword input and output)
    a.label("diff_start");
    a.emit(0xe92d_0ff0); // STMDB sp!,{r4-r11} (60 bytes total SVC stack use)
    a.emit(0xe310_0003); // TST r0,#3 (word-aligned header)
    a.branch(1, "invalid_argument");
    a.emit(0xe350_0402); // CMP r0,#0x02000000 (protected source range)
    a.branch(3, "invalid_argument");
    a.emit(0xe490_2004); // LDR r2,[r0],#4
    a.emit(0xe202_b0ff); // AND r11,r2,#0xff
    a.emit(0xe35c_0002); // CMP r12,#2
    a.emit(0x03a0_3082); // MOVEQ r3,#0x82 (16-bit differential header)
    a.emit(0x13a0_3081); // MOVNE r3,#0x81 (8-bit differential header)
    a.emit(0xe15b_0003); // CMP r11,r3
    a.branch(1, "invalid_argument");
    a.emit(0xe1a0_2422); // MOV r2,r2,LSR #8 (24-bit output byte count)
    a.emit(0xe35c_0000); // CMP r12,#0
    a.branch(0, "diff_validate_end");
    a.emit(0xe181_b002); // ORR r11,r1,r2 (even output address and byte count)
    a.emit(0xe31b_0001); // TST r11,#1
    a.branch(1, "invalid_argument");
    a.label("diff_validate_end");
    a.emit(0xe091_b002); // ADDS r11,r1,r2
    a.branch(2, "invalid_argument"); // Reject destination address wrap.
    a.emit(0xe090_b002); // ADDS r11,r0,r2
    a.branch(2, "invalid_argument"); // Reject source payload address wrap.
    a.emit(0xe352_0000); // CMP r2,#0
    a.branch(0, "diff_done");
    a.emit(0xe3a0_4000); // MOV r4,#0 (first delta is the first original sample)
    a.emit(0xe35c_0002); // CMP r12,#2
    a.branch(0, "diff_halfword");
    a.emit(0xe3a0_9000); // MOV r9,#0 (pending output halfword)
    a.emit(0xe3a0_a000); // MOV r10,#0 (pending byte position)

    a.label("diff_byte");
    a.emit(0xe4d0_5001); // LDRB r5,[r0],#1
    a.emit(0xe084_4005); // ADD r4,r4,r5
    a.emit(0xe204_40ff); // AND r4,r4,#255 (modulo 256)
    a.emit(0xe1a0_5004); // MOV r5,r4
    a.call("decompress_store_byte");
    a.emit(0xe352_0000); // CMP r2,#0 (writer consumed one byte)
    a.branch(1, "diff_byte");
    a.branch(14, "diff_done");

    a.label("diff_halfword");
    a.emit(0xe0d0_50b2); // LDRH r5,[r0],#2
    a.emit(0xe084_4005); // ADD r4,r4,r5
    a.emit(0xe0c1_40b2); // STRH r4,[r1],#2 (low 16 bits give modulo 65536)
    a.emit(0xe252_2002); // SUBS r2,r2,#2 (header length counts bytes, not samples)
    a.branch(1, "diff_halfword");
    a.label("diff_done");
    a.emit(0xe8bd_0ff0); // LDMIA sp!,{r4-r11}
    a.branch(14, "return");
}
