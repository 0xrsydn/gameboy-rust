//! Shared output routine for the original ARM decompression services.

use super::ArmImage;

/// Emit a leaf routine called with BL. r5 holds a zero-extended byte;
/// r1 is the destination, r2 the remaining output count, and r12 the write mode.
/// Halfword mode uses r9 as a pending value and r10 as bit position (0 or 8).
/// The routine changes flags, r1/r2, and r9/r10, but preserves the input byte.
pub(super) fn emit_writer(a: &mut ArmImage) {
    a.label("decompress_store_byte");
    a.emit(0xe35c_0000); // CMP r12,#0
    a.emit(0x05c1_5000); // STRBEQ r5,[r1] (normal byte-bus rules apply)
    a.branch(0, "decompress_stored");
    a.emit(0xe189_9a15); // ORR r9,r9,r5,LSL r10
    a.emit(0xe22a_a008); // EOR r10,r10,#8
    a.emit(0xe35a_0000); // CMP r10,#0
    a.emit(0x0141_90b1); // STRHEQ r9,[r1,#-1] (commit the completed halfword)
    a.emit(0x03a0_9000); // MOVEQ r9,#0
    a.label("decompress_stored");
    a.emit(0xe281_1001); // ADD r1,r1,#1
    a.emit(0xe242_2001); // SUB r2,r2,#1
    a.emit(0xe12f_ff1e); // BX lr
}
