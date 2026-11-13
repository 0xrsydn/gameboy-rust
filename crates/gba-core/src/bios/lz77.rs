//! Original LZ77 decoder emitted as ARM instructions.
//! Invalid streams trap after any earlier successful writes. VRAM output requires
//! an even size and halfword alignment for nonempty output; distance-one references
//! are rejected. A zero-length header returns without reading tokens or checking its type.
//!
//! Each step costs what mGBA's hardware-fitted model charges, besides its memory accesses:
//! 14 cycles per flag byte, 32 per literal or reference, and 10 per copied byte (14 for the
//! VRAM variant). The model also charges one more cycle per load than an ARM load's data
//! access, which the padding includes. The decoder returns r0 past the last source byte read, r1 past the output,
//! and r3 = 0, as mGBA reports.

use super::{pad, ArmImage};

#[derive(Clone, Copy, PartialEq)]
enum Output {
    Wram,
    Vram,
}

/// Fixed-part padding measured against mGBA with an original calibration ROM. The VRAM
/// variant's extra alignment checks leave it 8 cycles over mGBA per call.
const fn prologue_pad(output: Output) -> usize {
    match output {
        Output::Wram => 0,
        Output::Vram => 0,
    }
}

pub(super) fn emit(a: &mut ArmImage) {
    a.label("lz_wram");
    emit_decoder(a, Output::Wram);
    a.label("lz_vram");
    emit_decoder(a, Output::Vram);
}

fn emit_decoder(a: &mut ArmImage, output: Output) {
    let label = |name: &str| -> &'static str {
        const WRAM: [&str; 6] = [
            "lzw_done",
            "lzw_group",
            "lzw_block",
            "lzw_reference",
            "lzw_copy",
            "lzw_block_end",
        ];
        const VRAM: [&str; 6] = [
            "lzv_done",
            "lzv_group",
            "lzv_block",
            "lzv_reference",
            "lzv_copy",
            "lzv_block_end",
        ];
        let names = if output == Output::Wram { WRAM } else { VRAM };
        names
            .into_iter()
            .find(|full| full[4..] == *name)
            .expect("LZ77 label")
    };
    // r0 source, r1 output, r2 bytes left, r3 flags with a group-end sentinel, r4 byte,
    // r5 reference pointer, r6 run length, r7 output start, r8 VRAM pending byte, r12 scratch.
    a.emit(0xe92d_01f0); // STMDB sp!,{r4-r8} (48 bytes total SVC stack use)
    a.emit(0xe310_0003); // TST r0,#3 (header must be word-aligned)
    a.branch(1, "invalid_argument");
    a.emit(0xe350_0402); // CMP r0,#0x02000000 (do not decompress protected BIOS bytes)
    a.branch(3, "invalid_argument");
    a.emit(0xe490_2004); // LDR r2,[r0],#4 (header)
    a.emit(0xe202_c0ff); // AND r12,r2,#0xff
    a.emit(0xe1a0_2422); // MOV r2,r2,LSR #8 (24-bit output byte count)
    a.emit(0xe352_0000); // CMP r2,#0
    a.branch(0, label("done")); // Empty streams need neither a type tag nor a usable destination.
    a.emit(0xe35c_0010); // CMP r12,#0x10 (nonempty LZ77 type, reserved bits zero)
    a.branch(1, "invalid_argument");
    if output == Output::Vram {
        a.emit(0xe181_c002); // ORR r12,r1,r2 (both destination and size must be even)
        a.emit(0xe31c_0001); // TST r12,#1
        a.branch(1, "invalid_argument");
    }
    a.emit(0xe091_c002); // ADDS r12,r1,r2
    a.branch(2, "invalid_argument"); // Reject output address wrap.
    a.emit(0xe1a0_7001); // MOV r7,r1 (start of produced output)
    pad(a, prologue_pad(output));

    // Flag byte: 14 cycles plus its load (one more than the access), counting the
    // group-end branch below. Flags sit in bits 24-31 above a sentinel at bit 23.
    a.label(label("group"));
    a.emit(0xe4d0_3001); // LDRB r3,[r0],#1 (flags, most significant bit first)
    a.emit(0xe1a0_3c03); // MOV r3,r3,LSL #24
    a.emit(0xe383_3502); // ORR r3,r3,#0x00800000
    pad(a, 10);

    a.label(label("block"));
    a.emit(0xe313_0102); // TST r3,#0x80000000
    a.branch(1, label("reference"));
    // Literal: 32 cycles plus its load and store.
    a.emit(0xe4d0_4001); // LDRB r4,[r0],#1
    emit_store(a, output);
    a.emit(0xe242_2001); // SUB r2,r2,#1
    pad(a, if output == Output::Wram { 17 } else { 13 });
    a.branch(14, label("block_end"));

    // Reference: 32 cycles plus two loads, then 10 (VRAM: 14) per copied byte.
    a.label(label("reference"));
    a.emit(0xe4d0_4001); // LDRB r4,[r0],#1 (length nibble and high displacement bits)
    a.emit(0xe1a0_6224); // MOV r6,r4,LSR #4
    a.emit(0xe286_6003); // ADD r6,r6,#3 (length 3..18)
    a.emit(0xe204_500f); // AND r5,r4,#0xf
    a.emit(0xe4d0_4001); // LDRB r4,[r0],#1 (low displacement bits)
    a.emit(0xe184_5405); // ORR r5,r4,r5,LSL #8
    a.emit(0xe285_5001); // ADD r5,r5,#1 (distance 1..4096)
    a.emit(0xe041_c007); // SUB r12,r1,r7 (bytes already produced)
    a.emit(0xe155_000c); // CMP r5,r12
    a.branch(8, "invalid_argument"); // BHI: reference before start of output.
    a.emit(0xe156_0002); // CMP r6,r2
    a.branch(8, "invalid_argument"); // BHI: run would exceed the declared size.
    if output == Output::Vram {
        a.emit(0xe355_0001); // CMP r5,#1
        a.branch(0, "invalid_argument"); // VRAM firmware cannot use its uncommitted previous byte.
    }
    a.emit(0xe041_5005); // SUB r5,r1,r5 (reference pointer)
    pad(a, if output == Output::Wram { 10 } else { 8 });
    a.label(label("copy"));
    a.emit(0xe4d5_4001); // LDRB r4,[r5],#1 (overlaps observe newly produced data)
    emit_store(a, output);
    a.emit(0xe242_2001); // SUB r2,r2,#1
    a.emit(0xe256_6001); // SUBS r6,r6,#1
    pad(a, 3);
    a.branch(1, label("copy"));

    a.label(label("block_end"));
    a.emit(0xe1a0_3083); // MOV r3,r3,LSL #1
    a.emit(0xe352_0000); // CMP r2,#0
    a.branch(0, label("done"));
    a.emit(0xe353_0102); // CMP r3,#0x80000000 (sentinel reached bit 31: group done)
    a.branch(1, label("block"));
    a.branch(14, label("group"));

    a.label(label("done"));
    a.emit(0xe3a0_3000); // MOV r3,#0
    a.emit(0xe8bd_01f0); // LDMIA sp!,{r4-r8}
    super::emit_return_results(a); // r0: source end, r1: output end, r3: 0.
}

/// Write r4 at r1 and advance r1. WRAM stores bytes; VRAM pairs bytes into halfword stores,
/// costing the same whether or not the store executes, apart from the store itself.
fn emit_store(a: &mut ArmImage, output: Output) {
    match output {
        Output::Wram => a.emit(0xe4c1_4001), // STRB r4,[r1],#1
        Output::Vram => {
            a.emit(0xe311_0001); // TST r1,#1
            a.emit(0x1188_8404); // ORRNE r8,r8,r4,LSL #8
            a.emit(0x01a0_8004); // MOVEQ r8,r4
            a.emit(0x1141_80b1); // STRHNE r8,[r1,#-1] (commit the completed halfword)
            a.emit(0xe281_1001); // ADD r1,r1,#1
        }
    }
}
