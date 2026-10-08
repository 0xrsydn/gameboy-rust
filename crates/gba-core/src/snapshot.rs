//! Binary encoding primitives for save states.
//!
//! A state captures emulated hardware only. Configuration chosen by the host
//! (ROM, firmware image, cartridge hardware and save-device selection) is
//! supplied when the target machine is built and is checked, not restored.
//! Every multi-byte value is little-endian. The encoding is positional: a type
//! must read exactly what it wrote, in the same order. Section tags make a
//! misaligned reader fail at the next boundary instead of decoding garbage.

use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    /// The stream ended before a value or section was complete.
    Truncated,
    /// Bytes remain after the final section.
    TrailingData,
    /// The header does not identify a save state.
    BadMagic,
    /// The format version is not understood by this build.
    UnsupportedVersion(u32),
    /// The payload does not match its recorded checksum.
    Checksum,
    /// The state was captured with a different ROM, firmware, or cartridge setup.
    ConfigMismatch(&'static str),
    /// A section tag, enum value, or length is outside its valid range.
    Corrupt(&'static str),
    /// The machine is inside a CPU step; states exist only between steps.
    MidStep,
    /// The machine holds live state that has no encoding, such as a deferred
    /// diagnostic error. Saving at a later step boundary normally succeeds.
    Unsupported(&'static str),
    /// Decoding failed after the machine began to change. The machine state is
    /// unspecified and must be rebuilt. Valid states never produce this.
    RestoreFailed(Box<SnapshotError>),
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => write!(formatter, "save state is truncated"),
            Self::TrailingData => write!(formatter, "save state has trailing data"),
            Self::BadMagic => write!(formatter, "not a save state"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported save-state version {version}")
            }
            Self::Checksum => write!(formatter, "save state checksum mismatch"),
            Self::ConfigMismatch(what) => {
                write!(formatter, "save state does not match this machine: {what}")
            }
            Self::Corrupt(what) => write!(formatter, "corrupt save state: {what}"),
            Self::MidStep => write!(formatter, "cannot save state inside a CPU step"),
            Self::Unsupported(what) => write!(formatter, "cannot save state: {what}"),
            Self::RestoreFailed(cause) => {
                write!(
                    formatter,
                    "restore failed midway, machine must be rebuilt: {cause}"
                )
            }
        }
    }
}

impl Error for SnapshotError {}

/// Hardware state that survives in a save state.
///
/// `load` overwrites emulated state in place and keeps host configuration.
/// A failed `load` may leave `self` partly overwritten; callers validate the
/// whole stream (`Machine::load_state` does) before the first mutation.
pub(crate) trait Snapshot {
    fn save(&self, writer: &mut Writer);
    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError>;
}

#[derive(Debug, Default)]
pub(crate) struct Writer {
    bytes: Vec<u8>,
    error: Option<SnapshotError>,
}

impl Writer {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// The first recorded failure, or the encoded bytes.
    pub(crate) fn finish(self) -> Result<Vec<u8>, SnapshotError> {
        match self.error {
            Some(error) => Err(error),
            None => Ok(self.bytes),
        }
    }

    /// Record that the current state cannot be encoded. The first failure wins;
    /// callers may keep writing, and `finish` reports it.
    pub(crate) fn fail(&mut self, error: SnapshotError) {
        self.error.get_or_insert(error);
    }

    /// Four-byte section marker, checked by `Reader::tag`.
    pub(crate) fn tag(&mut self, tag: &[u8; 4]) {
        self.bytes.extend_from_slice(tag);
    }

    pub(crate) fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub(crate) fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    pub(crate) fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn i16(&mut self, value: i16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// Raw bytes with no length prefix. The reader must know the length.
    pub(crate) fn raw(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    /// Length-prefixed bytes.
    pub(crate) fn bytes(&mut self, bytes: &[u8]) {
        self.u32(u32::try_from(bytes.len()).expect("snapshot field exceeds 4 GiB"));
        self.raw(bytes);
    }

    /// `false`, or `true` followed by the value written by `save`.
    pub(crate) fn option<T>(&mut self, value: Option<&T>, save: impl FnOnce(&T, &mut Self)) {
        self.bool(value.is_some());
        if let Some(value) = value {
            save(value, self);
        }
    }
}

#[derive(Debug)]
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.bytes.len()
    }

    /// Fails with `TrailingData` unless every byte was consumed.
    pub(crate) fn finish(&self) -> Result<(), SnapshotError> {
        if self.bytes.is_empty() {
            Ok(())
        } else {
            Err(SnapshotError::TrailingData)
        }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], SnapshotError> {
        if self.bytes.len() < length {
            return Err(SnapshotError::Truncated);
        }
        let (head, tail) = self.bytes.split_at(length);
        self.bytes = tail;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], SnapshotError> {
        let mut array = [0; N];
        array.copy_from_slice(self.take(N)?);
        Ok(array)
    }

    pub(crate) fn tag(&mut self, expected: &[u8; 4]) -> Result<(), SnapshotError> {
        if self.take(4)? == expected {
            Ok(())
        } else {
            Err(SnapshotError::Corrupt("section tag"))
        }
    }

    pub(crate) fn u8(&mut self) -> Result<u8, SnapshotError> {
        Ok(self.array::<1>()?[0])
    }

    pub(crate) fn bool(&mut self) -> Result<bool, SnapshotError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(SnapshotError::Corrupt("boolean")),
        }
    }

    pub(crate) fn u16(&mut self) -> Result<u16, SnapshotError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub(crate) fn i16(&mut self) -> Result<i16, SnapshotError> {
        Ok(i16::from_le_bytes(self.array()?))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, SnapshotError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, SnapshotError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    /// Fill `destination` from bytes written by `Writer::raw`.
    pub(crate) fn raw_into(&mut self, destination: &mut [u8]) -> Result<(), SnapshotError> {
        destination.copy_from_slice(self.take(destination.len())?);
        Ok(())
    }

    /// Fill `destination` from bytes written by `Writer::bytes`. The recorded
    /// length must equal `destination.len()`; fixed-size memories never resize.
    pub(crate) fn bytes_into(&mut self, destination: &mut [u8]) -> Result<(), SnapshotError> {
        let length = self.u32()? as usize;
        if length != destination.len() {
            return Err(SnapshotError::Corrupt("memory size"));
        }
        self.raw_into(destination)
    }

    /// Length-prefixed bytes as a vector, rejecting lengths above `limit`.
    pub(crate) fn bytes_vec(&mut self, limit: usize) -> Result<Vec<u8>, SnapshotError> {
        let length = self.u32()? as usize;
        if length > limit {
            return Err(SnapshotError::Corrupt("length"));
        }
        Ok(self.take(length)?.to_vec())
    }

    pub(crate) fn option<T>(
        &mut self,
        load: impl FnOnce(&mut Self) -> Result<T, SnapshotError>,
    ) -> Result<Option<T>, SnapshotError> {
        if self.bool()? {
            load(self).map(Some)
        } else {
            Ok(None)
        }
    }
}

/// FNV-1a over `bytes`; identifies images and detects accidental corruption.
/// It is not a security boundary.
pub(crate) fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_in_order() {
        let mut writer = Writer::new();
        writer.tag(b"TEST");
        writer.u8(0xab);
        writer.bool(true);
        writer.u16(0xbeef);
        writer.i16(-2);
        writer.u32(0xdead_beef);
        writer.u64(u64::MAX - 1);
        writer.bytes(&[1, 2, 3]);
        writer.option(Some(&7u16), |value, writer| writer.u16(*value));
        writer.option(None::<&u16>, |value, writer| writer.u16(*value));
        let bytes = writer.finish().unwrap();

        let mut reader = Reader::new(&bytes);
        reader.tag(b"TEST").unwrap();
        assert_eq!(reader.u8().unwrap(), 0xab);
        assert!(reader.bool().unwrap());
        assert_eq!(reader.u16().unwrap(), 0xbeef);
        assert_eq!(reader.i16().unwrap(), -2);
        assert_eq!(reader.u32().unwrap(), 0xdead_beef);
        assert_eq!(reader.u64().unwrap(), u64::MAX - 1);
        let mut three = [0; 3];
        reader.bytes_into(&mut three).unwrap();
        assert_eq!(three, [1, 2, 3]);
        assert_eq!(reader.option(Reader::u16).unwrap(), Some(7));
        assert_eq!(reader.option(Reader::u16).unwrap(), None);
        reader.finish().unwrap();
    }

    #[test]
    fn first_recorded_failure_is_reported() {
        let mut writer = Writer::new();
        writer.fail(SnapshotError::Unsupported("first"));
        writer.u8(1);
        writer.fail(SnapshotError::MidStep);
        assert_eq!(writer.finish(), Err(SnapshotError::Unsupported("first")));
    }

    #[test]
    fn malformed_input_is_rejected_without_panicking() {
        assert_eq!(Reader::new(&[1]).u16(), Err(SnapshotError::Truncated));
        assert_eq!(
            Reader::new(&[2]).bool(),
            Err(SnapshotError::Corrupt("boolean"))
        );
        assert_eq!(
            Reader::new(b"NOPE").tag(b"TEST"),
            Err(SnapshotError::Corrupt("section tag"))
        );
        assert_eq!(Reader::new(&[0]).finish(), Err(SnapshotError::TrailingData));
        // A length prefix larger than the stream or the destination fails cleanly.
        let mut huge = Writer::new();
        huge.u32(u32::MAX);
        let huge = huge.finish().unwrap();
        assert_eq!(
            Reader::new(&huge).bytes_vec(1 << 20),
            Err(SnapshotError::Corrupt("length"))
        );
        assert_eq!(
            Reader::new(&huge).bytes_into(&mut [0; 4]),
            Err(SnapshotError::Corrupt("memory size"))
        );
    }
}
