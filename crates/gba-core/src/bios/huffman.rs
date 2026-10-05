//! Original Huffman decoder emitted as ARM instructions.
//! Supports 4/8-bit symbols and complete aligned output words.
//! Only traversed tree edges/leaves are validated; invalid inputs are diagnostics.

use super::ArmImage;

pub(super) fn emit(a: &mut ArmImage) {
    a.label("huffman");
    // The common SWI frame already saved lr. This leaf routine uses it as scratch.
    a.emit(0xe92d_0ff0); // STMDB sp!,{r4-r11} (60 bytes total SVC stack use)
    a.emit(0xe180_b001); // ORR r11,r0,r1
    a.emit(0xe31b_0003); // TST r11,#3 (word-aligned source and destination)
    a.branch(1, "invalid_argument");
    a.emit(0xe350_0402); // CMP r0,#0x02000000
    a.branch(3, "invalid_argument");
    a.emit(0xe490_2004); // LDR r2,[r0],#4 (header)
    a.emit(0xe202_b0ff); // AND r11,r2,#0xff
    a.emit(0xe35b_0024); // CMP r11,#0x24
    a.branch(0, "huff_header");
    a.emit(0xe35b_0028); // CMP r11,#0x28
    a.branch(1, "invalid_argument");
    a.label("huff_header");
    a.emit(0xe20b_300f); // AND r3,r11,#15 (symbol width)
    a.emit(0xe1a0_2422); // MOV r2,r2,LSR #8 (output byte count)
    a.emit(0xe312_0003); // TST r2,#3 (complete output words)
    a.branch(1, "invalid_argument");
    a.emit(0xe091_b002); // ADDS r11,r1,r2
    a.branch(2, "invalid_argument"); // Output address wrap.
    a.emit(0xe352_0000); // CMP r2,#0
    a.branch(0, "huff_done"); // No tree or bitstream access for zero output.

    a.emit(0xe4d0_c001); // LDRB r12,[r0],#1 (tree size byte)
    a.emit(0xe1a0_4000); // MOV r4,r0 (root at source+5)
    a.emit(0xe1a0_c08c); // MOV r12,r12,LSL #1
    a.emit(0xe28c_c001); // ADD r12,r12,#1 (table bytes excluding size byte)
    a.emit(0xe094_500c); // ADDS r5,r4,r12 (exclusive tree end / bitstream start)
    a.branch(2, "invalid_argument");
    a.emit(0xe315_0003); // TST r5,#3 (padded tree must give aligned input words)
    a.branch(1, "invalid_argument");
    a.emit(0xe1a0_0005); // MOV r0,r5 (bitstream cursor)
    a.emit(0xe1a0_6004); // MOV r6,r4 (current node)
    a.emit(0xe3a0_8000); // MOV r8,#0 (remaining input bits)
    a.emit(0xe3a0_9000); // MOV r9,#0 (pending output word)
    a.emit(0xe3a0_a000); // MOV r10,#0 (output bit position)

    a.label("huff_bit");
    a.emit(0xe358_0000); // CMP r8,#0
    a.emit(0x0490_7004); // LDREQ r7,[r0],#4 (little-endian input word)
    a.emit(0x03a0_8020); // MOVEQ r8,#32
    a.emit(0xe5d6_b000); // LDRB r11,[r6] (current node descriptor)
    a.emit(0xe20b_c03f); // AND r12,r11,#63
    a.emit(0xe28c_c001); // ADD r12,r12,#1
    a.emit(0xe3c6_e001); // BIC lr,r6,#1
    a.emit(0xe09e_c08c); // ADDS r12,lr,r12,LSL #1 (left child address)
    a.branch(2, "invalid_argument"); // Reject child address wrap.
    a.emit(0xe1b0_7087); // MOVS r7,r7,LSL #1 (old bit31 becomes carry)
    a.emit(0x228c_c001); // ADDCS r12,r12,#1 (right child)
    a.emit(0x33a0_e080); // MOVCC lr,#0x80 (left leaf flag)
    a.emit(0x23a0_e040); // MOVCS lr,#0x40 (right leaf flag)
    a.emit(0xe248_8001); // SUB r8,r8,#1 (consume the branch bit)
    a.emit(0xe155_000c); // CMP r5,r12
    a.branch(9, "invalid_argument"); // BLS: child >= exclusive tree end.

    // Children always advance, so bounds checks also bound traversal depth.
    a.emit(0xe11b_000e); // TST r11,lr (selected child is a leaf?)
    a.branch(0, "huff_node");
    a.emit(0xe5dc_b000); // LDRB r11,[r12] (symbol)
    a.emit(0xe1a0_e33b); // MOV lr,r11,LSR r3
    a.emit(0xe35e_0000); // CMP lr,#0 (reject upper bits in 4-bit leaves)
    a.branch(1, "invalid_argument");
    a.emit(0xe189_9a1b); // ORR r9,r9,r11,LSL r10
    a.emit(0xe08a_a003); // ADD r10,r10,r3
    a.emit(0xe1a0_6004); // MOV r6,r4 (restart at root after each symbol)
    a.emit(0xe35a_0020); // CMP r10,#32
    a.branch(1, "huff_bit");
    a.emit(0xe481_9004); // STR r9,[r1],#4 (commit completed word)
    a.emit(0xe252_2004); // SUBS r2,r2,#4
    a.branch(0, "huff_done");
    a.emit(0xe3a0_9000); // MOV r9,#0
    a.emit(0xe3a0_a000); // MOV r10,#0
    a.branch(14, "huff_bit");
    a.label("huff_node");
    a.emit(0xe1a0_600c); // MOV r6,r12
    a.branch(14, "huff_bit");
    a.label("huff_done");
    a.emit(0xe8bd_0ff0); // LDMIA sp!,{r4-r11}
    a.branch(14, "return");
}
