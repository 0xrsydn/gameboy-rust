//! Whole-machine save states.
//!
//! Layout: `GBAS`, format version, ROM length and fingerprint, firmware
//! presence and fingerprint, cartridge setup, payload length, payload, then a
//! fingerprint of every preceding byte. The payload is CPU, memory bus (with all
//! devices), and last step timing, each in the positional encoding of
//! [`crate::snapshot`].

use super::Machine;
use crate::snapshot::{fingerprint, Reader, Snapshot, SnapshotError, Writer};

const MAGIC: &[u8; 4] = b"GBAS";
/// Bump on any change to an encoded type; older states are rejected, not migrated.
const VERSION: u32 = 8;
const CHECKSUM_LEN: usize = 8;

impl Machine {
    /// Encode the emulated hardware at the current step boundary.
    ///
    /// The state holds no ROM or firmware bytes: it records their fingerprints and
    /// can be restored only into a machine built from the same images and the same
    /// cartridge hardware/save-device selection. Fails with `MidStep` inside a step
    /// and `Unsupported` when a deferred diagnostic cannot be encoded.
    pub fn save_state(&self) -> Result<Vec<u8>, SnapshotError> {
        let mut payload = Writer::new();
        self.cpu.save(&mut payload);
        self.memory.save(&mut payload);
        save_timing(&self.last_timing, &mut payload);
        let payload = payload.finish()?;

        let mut writer = Writer::new();
        writer.raw(MAGIC);
        writer.u32(VERSION);
        self.write_identity(&mut writer);
        writer.bytes(&payload);
        let mut bytes = writer.finish()?;
        let checksum = fingerprint(&bytes);
        bytes.extend_from_slice(&checksum.to_le_bytes());
        Ok(bytes)
    }

    /// Restore a state produced by [`Machine::save_state`].
    ///
    /// The format, checksum, and image/cartridge identity are all verified before
    /// any machine state changes, so every such failure leaves the machine intact.
    /// A later decoding error is reported as `RestoreFailed`: it indicates an encoder
    /// defect, and the machine is then unspecified and must be rebuilt from the ROM.
    /// Host configuration (ROM, firmware, cartridge setup, row-capture and
    /// audio-capture enablement) is kept; capture queues and counters are cleared.
    pub fn load_state(&mut self, bytes: &[u8]) -> Result<(), SnapshotError> {
        if bytes.len() < MAGIC.len() + 4 + CHECKSUM_LEN {
            return Err(SnapshotError::Truncated);
        }
        if &bytes[..MAGIC.len()] != MAGIC {
            return Err(SnapshotError::BadMagic);
        }
        let mut reader = Reader::new(&bytes[MAGIC.len()..]);
        let version = reader.u32()?;
        if version != VERSION {
            return Err(SnapshotError::UnsupportedVersion(version));
        }
        let (body, checksum) = bytes.split_at(bytes.len() - CHECKSUM_LEN);
        if fingerprint(body).to_le_bytes() != checksum {
            return Err(SnapshotError::Checksum);
        }
        let mut reader = Reader::new(&body[MAGIC.len() + 4..]);
        self.check_identity(&mut reader)?;
        let payload = reader.bytes_vec(body.len())?;
        reader.finish()?;

        let mut payload = Reader::new(&payload);
        self.restore(&mut payload)
            .map_err(|cause| SnapshotError::RestoreFailed(Box::new(cause)))
    }

    fn restore(&mut self, payload: &mut Reader<'_>) -> Result<(), SnapshotError> {
        self.cpu.load(payload)?;
        self.memory.load(payload)?;
        self.last_timing = load_timing(payload)?;
        payload.finish()
    }

    fn write_identity(&self, writer: &mut Writer) {
        let (rom, bios) = self.memory.images();
        writer.u64(rom.len() as u64);
        writer.u64(fingerprint(rom));
        writer.option(bios.as_ref(), |bios, writer| {
            writer.u64(fingerprint(bios));
        });
        writer.raw(&self.memory.setup_identity());
    }

    fn check_identity(&self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let (rom, bios) = self.memory.images();
        if reader.u64()? != rom.len() as u64 || reader.u64()? != fingerprint(rom) {
            return Err(SnapshotError::ConfigMismatch("ROM image"));
        }
        let saved_bios = reader.option(Reader::u64)?;
        if saved_bios != bios.map(fingerprint) {
            return Err(SnapshotError::ConfigMismatch("firmware image"));
        }
        let mut setup = [0; 2];
        reader.raw_into(&mut setup)?;
        if setup != self.memory.setup_identity() {
            return Err(SnapshotError::ConfigMismatch("cartridge setup"));
        }
        Ok(())
    }
}

fn save_timing(timing: &crate::timing::StepTiming, writer: &mut Writer) {
    let crate::timing::StepTiming {
        code_cycles,
        data_cycles,
        internal_cycles,
        idle_cycles,
    } = timing;
    writer.u32(*code_cycles);
    writer.u32(*data_cycles);
    writer.u32(*internal_cycles);
    writer.u32(*idle_cycles);
}

fn load_timing(reader: &mut Reader<'_>) -> Result<crate::timing::StepTiming, SnapshotError> {
    Ok(crate::timing::StepTiming {
        code_cycles: reader.u32()?,
        data_cycles: reader.u32()?,
        internal_cycles: reader.u32()?,
        idle_cycles: reader.u32()?,
    })
}
