//! Opt-in desktop audio. The emulator core never owns a host device.
#[cfg(target_os = "macos")]
mod device;
#[cfg(any(target_os = "macos", test))]
mod playback;
#[cfg(target_os = "macos")]
pub use device::AudioOutput;

#[cfg(not(target_os = "macos"))]
pub struct AudioOutput;
#[cfg(not(target_os = "macos"))]
impl AudioOutput {
    pub fn new() -> std::io::Result<Self> {
        Err(std::io::Error::other(
            "--audio currently requires macOS; omit --audio to run muted",
        ))
    }
    pub fn submit(&self, _: &[gba_core::audio::StereoLevel]) -> std::io::Result<()> {
        unreachable!()
    }
    pub fn clear(&self) -> std::io::Result<()> {
        unreachable!()
    }
    pub fn description(&self) -> String {
        unreachable!()
    }
    pub fn report(&self, _: &mut impl std::io::Write) -> std::io::Result<()> {
        unreachable!()
    }
}
