//! Save-state encoding for the memory bus and everything it owns.

use super::Memory;
use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};
use crate::video::capture::Capture;

impl Memory {
    /// ROM and firmware bytes, for state identity checks.
    pub(crate) fn images(&self) -> (&[u8], Option<&[u8]>) {
        (&self.rom, self.bios.as_deref())
    }

    /// Cartridge hardware and save-device selection, which a state must match.
    pub(crate) fn setup_identity(&self) -> [u8; 2] {
        self.cartridge.setup_identity()
    }

    /// Staged per-step transactions exist only inside `Machine::step`.
    fn mid_step(&self) -> bool {
        self.step_active.get()
            || self.cartridge_step.get().is_some()
            || self.timer_step.get().is_some()
            || self.cpu_timing.get().is_some()
            || self.cpu_access.is_some()
            || self.cartridge_elapsed.get() != 0
    }
}

impl Snapshot for Memory {
    fn save(&self, writer: &mut Writer) {
        if self.mid_step() {
            writer.fail(SnapshotError::MidStep);
            return;
        }
        let Self {
            external_ram,
            internal_ram,
            palette_ram,
            video_ram,
            oam,
            rom: _, // Host image, identified by the state header.
            save_image,
            save_modified,
            cartridge_elapsed: _, // Zero between steps; checked above.
            cartridge,
            cartridge_step: _, // Staged transaction; empty between steps.
            bios: _,           // Host image, identified by the state header.
            io,
            cycles,
            cpu_timing: _,   // Staged transaction; empty between steps.
            timer_step: _,   // Staged transaction; empty between steps.
            step_active: _,  // False between steps; checked above.
            step_elapsed: _, // Zero between steps.
            gamepak_prefetch,
            #[cfg(test)]
                last_cpu_timing: _, // Test trace only.
            cpu_resume_nonsequential,
            cpu_access: _, // Staged transaction; empty between steps.
            bios_prefetch,
            iwram_bus,
            scanline_capture,
            sprite_pipeline,
        } = self;
        writer.tag(b"MEM0");
        // The cartridge comes first so a differing save device reports a
        // configuration mismatch before any memory-size error.
        cartridge.save(writer);
        writer.bytes(external_ram);
        writer.bytes(internal_ram);
        writer.bytes(palette_ram);
        writer.bytes(video_ram);
        writer.bytes(oam);
        writer.bytes(save_image);
        writer.bool(*save_modified);
        io.save(writer);
        writer.u64(*cycles);
        gamepak_prefetch.save(writer);
        writer.bool(*cpu_resume_nonsequential);
        writer.option(bios_prefetch.as_ref(), |value, writer| writer.u32(*value));
        iwram_bus.save(writer);
        writer.option(scanline_capture.as_ref(), |capture, writer| {
            capture.save(writer);
        });
        sprite_pipeline.save(writer);
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        if self.mid_step() {
            return Err(SnapshotError::MidStep);
        }
        let Self {
            external_ram,
            internal_ram,
            palette_ram,
            video_ram,
            oam,
            rom: _,
            save_image,
            save_modified,
            cartridge_elapsed: _,
            cartridge,
            cartridge_step: _,
            bios: _,
            io,
            cycles,
            cpu_timing: _,
            timer_step: _,
            step_active: _,
            step_elapsed: _,
            gamepak_prefetch,
            #[cfg(test)]
                last_cpu_timing: _,
            cpu_resume_nonsequential,
            cpu_access: _,
            bios_prefetch,
            iwram_bus,
            scanline_capture,
            sprite_pipeline,
        } = self;
        reader.tag(b"MEM0")?;
        cartridge.load(reader)?;
        reader.bytes_into(external_ram)?;
        reader.bytes_into(internal_ram)?;
        reader.bytes_into(palette_ram)?;
        reader.bytes_into(video_ram)?;
        reader.bytes_into(oam)?;
        reader.bytes_into(save_image)?;
        *save_modified = reader.bool()?;
        io.load(reader)?;
        *cycles = reader.u64()?;
        gamepak_prefetch.load(reader)?;
        *cpu_resume_nonsequential = reader.bool()?;
        *bios_prefetch = reader.option(Reader::u32)?;
        iwram_bus.load(reader)?;
        // Whether row capture runs is host configuration; only its contents are state.
        let captured = reader.option(|reader| {
            let mut capture = Capture::default();
            capture.load(reader)?;
            Ok(capture)
        })?;
        if scanline_capture.is_some() {
            *scanline_capture = Some(captured.unwrap_or_default());
        }
        sprite_pipeline.load(reader)?;
        Ok(())
    }
}
