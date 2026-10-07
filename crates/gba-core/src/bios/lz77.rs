//! Original LZ77 decoder emitted as ARM instructions.
//! Invalid streams trap after any earlier successful writes. VRAM output requires
//! an even size and halfword alignment for nonempty output; distance-one references
//! are rejected. A zero-length header returns without reading tokens or checking its type.

use super::ArmImage;

pub(super) fn emit(a: &mut ArmImage) {
    a.label("lz_wram");
    a.emit(0xe3a0_c000); // MOV r12,#0 (byte stores)
    a.branch(14, "lz_start");
    a.label("lz_vram");
    a.emit(0xe3a0_c001); // MOV r12,#1 (buffer bytes into halfword stores)
    a.label("lz_start");
    a.emit(0xe92d_0ff0); // STMDB sp!,{r4-r11} (60 bytes total SVC stack use)
    a.emit(0xe310_0003); // TST r0,#3 (header must be word-aligned)
    a.branch(1, "invalid_argument");
    a.emit(0xe350_0402); // CMP r0,#0x02000000 (do not decompress protected BIOS bytes)
    a.branch(3, "invalid_argument");
    a.emit(0xe490_2004); // LDR r2,[r0],#4 (header)
    a.emit(0xe202_b0ff); // AND r11,r2,#0xff
    a.emit(0xe1a0_2422); // MOV r2,r2,LSR #8 (24-bit output byte count)
    a.emit(0xe352_0000); // CMP r2,#0
    a.branch(0, "lz_done"); // Empty streams need neither a type tag nor a usable destination.
    a.emit(0xe35b_0010); // CMP r11,#0x10 (nonempty LZ77 type, reserved bits zero)
    a.branch(1, "invalid_argument");
    a.emit(0xe35c_0000); // CMP r12,#0
    a.branch(0, "lz_validate_end");
    a.emit(0xe181_b002); // ORR r11,r1,r2 (both destination and size must be even)
    a.emit(0xe31b_0001); // TST r11,#1
    a.branch(1, "invalid_argument");
    a.label("lz_validate_end");
    a.emit(0xe091_b002); // ADDS r11,r1,r2
    a.branch(2, "invalid_argument"); // Reject output address wrap.
    a.emit(0xe1a0_8001); // MOV r8,r1 (start of produced output)
    a.emit(0xe3a0_9000); // MOV r9,#0 (pending halfword)
    a.emit(0xe3a0_a000); // MOV r10,#0 (bit position 0 or 8)

    a.label("lz_group");
    a.emit(0xe4d0_3001); // LDRB r3,[r0],#1 (flags, most significant bit first)
    a.emit(0xe3a0_4080); // MOV r4,#0x80
    a.label("lz_token");
    a.emit(0xe113_0004); // TST r3,r4
    a.branch(1, "lz_reference");
    a.emit(0xe4d0_5001); // LDRB r5,[r0],#1 (literal)
    a.call("decompress_store_byte");
    a.branch(14, "lz_next_token");

    a.label("lz_reference");
    a.emit(0xe4d0_5001); // LDRB r5,[r0],#1 (length nibble and high displacement bits)
    a.emit(0xe1a0_7225); // MOV r7,r5,LSR #4
    a.emit(0xe287_7003); // ADD r7,r7,#3 (length 3..18)
    a.emit(0xe205_600f); // AND r6,r5,#0xf
    a.emit(0xe4d0_5001); // LDRB r5,[r0],#1 (low displacement bits)
    a.emit(0xe185_6406); // ORR r6,r5,r6,LSL #8
    a.emit(0xe286_6001); // ADD r6,r6,#1 (distance 1..4096)
    a.emit(0xe041_b008); // SUB r11,r1,r8 (bytes already produced)
    a.emit(0xe156_000b); // CMP r6,r11
    a.branch(8, "invalid_argument"); // BHI: reference before start of output.
    a.emit(0xe157_0002); // CMP r7,r2
    a.branch(8, "invalid_argument"); // BHI: run would exceed the declared size.
    a.emit(0xe35c_0000); // CMP r12,#0
    a.branch(0, "lz_distance_ok");
    a.emit(0xe356_0001); // CMP r6,#1
    a.branch(0, "invalid_argument"); // VRAM firmware cannot use its uncommitted previous byte.
    a.label("lz_distance_ok");
    a.emit(0xe041_6006); // SUB r6,r1,r6 (reference pointer)
    a.label("lz_reference_byte");
    a.emit(0xe4d6_5001); // LDRB r5,[r6],#1 (overlaps observe newly produced data)
    a.call("decompress_store_byte");
    a.emit(0xe257_7001); // SUBS r7,r7,#1
    a.branch(1, "lz_reference_byte");

    a.label("lz_next_token");
    a.emit(0xe352_0000); // CMP r2,#0
    a.branch(0, "lz_done");
    a.emit(0xe1b0_40a4); // MOVS r4,r4,LSR #1
    a.branch(1, "lz_token");
    a.branch(14, "lz_group");
    a.label("lz_done");
    a.emit(0xe8bd_0ff0); // LDMIA sp!,{r4-r11}
    a.branch(14, "return");
}
