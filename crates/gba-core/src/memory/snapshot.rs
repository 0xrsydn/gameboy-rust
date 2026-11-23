//! Save-state encoding for the memory bus and everything it owns.

use super::Memory;
use crate::snapshot::{fingerprint, Reader, Snapshot, SnapshotError, Writer};
use crate::video::capture::Capture;

impl Memory {
    /// ROM length and the ROM and firmware fingerprints, for state identity checks.
    pub(crate) fn image_identity(&self) -> (u64, u64, Option<u64>) {
        let (rom, bios) = *self.image_fingerprints.get_or_init(|| {
            (
                fingerprint(&self.rom),
                self.bios.as_deref().map(fingerprint),
            )
        });
        (self.rom.len() as u64, rom, bios)
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
            cartridge_step: _,     // Staged transaction; empty between steps.
            bios: _,               // Host image, identified by the state header.
            image_fingerprints: _, // Derived from the images.
            io,
            cycles,
            cpu_timing: _,   // Staged transaction; empty between steps.
            timer_step: _,   // Staged transaction; empty between steps.
            step_active: _,  // False between steps; checked above.
            step_elapsed: _, // Zero between steps.
            gamepak_prefetch,
            #[cfg(test)]
                last_cpu_timing: _, // Test trace only.
            #[cfg(test)]
                general_steps_only: _, // Test switch.
            #[cfg(test)]
                quiet_steps: _, // Test counter.
            cpu_resume_nonsequential,
            cpu_access: _,    // Staged transaction; empty between steps.
            dma_units: _,     // Set only while a DMA unit writes.
            dma_from_bios: _, // Set by the machine before each DMA unit.
            dma_bus,
            bios_prefetch,
            strict_rom_padding: _, // Host option, kept across states.
            iwram_bus,
            scanline_capture,
            sprite_pipeline,
            // A diagnostic counter for idle-loop detection, not machine state.
            side_accesses: _,
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
        writer.option(dma_bus.as_ref(), |value, writer| writer.u32(*value));
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
            image_fingerprints: _,
            io,
            cycles,
            cpu_timing: _,
            timer_step: _,
            step_active: _,
            step_elapsed: _,
            gamepak_prefetch,
            #[cfg(test)]
                last_cpu_timing: _,
            #[cfg(test)]
                general_steps_only: _,
            #[cfg(test)]
                quiet_steps: _,
            cpu_resume_nonsequential,
            cpu_access: _,
            dma_units: _,
            dma_from_bios: _,
            dma_bus,
            bios_prefetch,
            strict_rom_padding: _, // Host option, kept across states.
            iwram_bus,
            scanline_capture,
            sprite_pipeline,
            side_accesses: _,
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
        *dma_bus = reader.option(Reader::u32)?;
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
