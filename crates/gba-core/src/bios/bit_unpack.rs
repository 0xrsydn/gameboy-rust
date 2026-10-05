//! Original BitUnPack routine emitted as ARM instructions.
//! This subset requires widening/equal widths and complete output words.
//! Invalid parameters or overflowing offset values produce development diagnostics.

use super::ArmImage;

pub(super) fn emit(a: &mut ArmImage) {
    a.label("bit_unpack");
    a.emit(0xe92d_0ff0); // STMDB sp!,{r4-r11} (60 bytes total SVC stack use)
    a.emit(0xe181_c002); // ORR r12,r1,r2
    a.emit(0xe31c_0003); // TST r12,#3 (word-aligned destination and info)
    a.branch(1, "invalid_argument");
    a.emit(0xe350_0402); // CMP r0,#0x02000000 (protected source range)
    a.branch(3, "invalid_argument");
    a.emit(0xe1d2_30b0); // LDRH r3,[r2] (source byte count)
    a.emit(0xe5d2_4002); // LDRB r4,[r2,#2] (source width)
    a.emit(0xe5d2_5003); // LDRB r5,[r2,#3] (destination width)
    a.emit(0xe592_6004); // LDR r6,[r2,#4] (offset and zero-data flag)

    a.emit(0xe354_0000); // CMP r4,#0
    a.branch(0, "invalid_argument");
    a.emit(0xe354_0008); // CMP r4,#8
    a.branch(8, "invalid_argument");
    a.emit(0xe244_c001); // SUB r12,r4,#1
    a.emit(0xe114_000c); // TST r4,r12 (power of two)
    a.branch(1, "invalid_argument");
    a.emit(0xe155_0004); // CMP r5,r4 (reject narrowing, including zero width)
    a.branch(3, "invalid_argument");
    a.emit(0xe355_0020); // CMP r5,#32
    a.branch(8, "invalid_argument");
    a.emit(0xe245_c001); // SUB r12,r5,#1
    a.emit(0xe115_000c); // TST r5,r12
    a.branch(1, "invalid_argument");

    // Output bytes = source bytes * destination width / source width.
    // Validated powers of two make the division exact, with at most three shifts.
    a.emit(0xe00c_0593); // MUL r12,r3,r5
    a.emit(0xe3a0_2001); // MOV r2,#1
    a.label("unpack_size");
    a.emit(0xe152_0004); // CMP r2,r4
    a.branch(0, "unpack_validate_end");
    a.emit(0xe1a0_c0ac); // MOV r12,r12,LSR #1
    a.emit(0xe1a0_2082); // MOV r2,r2,LSL #1
    a.branch(14, "unpack_size");
    a.label("unpack_validate_end");
    a.emit(0xe31c_0003); // TST r12,#3 (no partial output word)
    a.branch(1, "invalid_argument");
    a.emit(0xe091_200c); // ADDS r2,r1,r12
    a.branch(2, "invalid_argument"); // Output address wrap.
    a.emit(0xe090_2003); // ADDS r2,r0,r3
    a.branch(2, "invalid_argument"); // Source address wrap.
    a.emit(0xe353_0000); // CMP r3,#0
    a.branch(0, "unpack_done");
    a.emit(0xe3a0_7001); // MOV r7,#1
    a.emit(0xe1a0_7417); // MOV r7,r7,LSL r4
    a.emit(0xe247_7001); // SUB r7,r7,#1 (source mask)
    a.emit(0xe3a0_9000); // MOV r9,#0 (pending word)
    a.emit(0xe3a0_a000); // MOV r10,#0 (output bit position)

    a.label("unpack_byte");
    a.emit(0xe4d0_8001); // LDRB r8,[r0],#1
    a.emit(0xe3a0_b008); // MOV r11,#8 (remaining input bits)
    a.label("unpack_unit");
    a.emit(0xe008_c007); // AND r12,r8,r7
    a.emit(0xe35c_0000); // CMP r12,#0
    a.branch(1, "unpack_offset");
    a.emit(0xe316_0102); // TST r6,#0x80000000 (also offset zero units?)
    a.branch(0, "unpack_value");
    a.label("unpack_offset");
    a.emit(0xe3c6_2102); // BIC r2,r6,#0x80000000
    a.emit(0xe08c_c002); // ADD r12,r12,r2
    a.label("unpack_value");
    a.emit(0xe1a0_253c); // MOV r2,r12,LSR r5 (register shift by 32 gives zero)
    a.emit(0xe352_0000); // CMP r2,#0
    a.branch(1, "invalid_argument"); // Converted value must fit destination width.
    a.emit(0xe189_9a1c); // ORR r9,r9,r12,LSL r10
    a.emit(0xe08a_a005); // ADD r10,r10,r5
    a.emit(0xe35a_0020); // CMP r10,#32
    a.emit(0x0481_9004); // STREQ r9,[r1],#4
    a.emit(0x03a0_9000); // MOVEQ r9,#0
    a.emit(0x03a0_a000); // MOVEQ r10,#0
    a.emit(0xe1a0_8438); // MOV r8,r8,LSR r4
    a.emit(0xe05b_b004); // SUBS r11,r11,r4
    a.branch(1, "unpack_unit");
    a.emit(0xe253_3001); // SUBS r3,r3,#1
    a.branch(1, "unpack_byte");
    a.label("unpack_done");
    a.emit(0xe8bd_0ff0); // LDMIA sp!,{r4-r11}
    a.branch(14, "return");
}
