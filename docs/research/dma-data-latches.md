# DMA retained data and CPU bus ownership

## Finding

DMA channel data, IWRAM lane history, and the CPU's general bus value are different state.
A channel's retained data cannot safely replace every open-bus read after a transfer.
The previous implementation rejected all DMA sources below work RAM, even after the channel had transferred known data.
That diagnostic omitted a documented retained-data case.

The [NanoBoyAdvance hardware latch test](https://codeberg.org/nba-emu/hw-test/src/commit/cc3f4a286cdef823980d9b353bd70befc9927d28/dma/latch/source/main.c) specifies these expectations:

- Each DMA channel retains its own 32-bit value.
- A mapped word source replaces that value.
- A mapped halfword source duplicates its value in both retained halfwords.
- A blocked BIOS source does not replace the retained value.
- A halfword write from a blocked source selects the retained lane using destination bit 1.
- A write-only I/O source can read ordinary bus history, not simply the previous channel value.

The test explicitly exercises source zero. Its comments extend the blocked-source rule below `0x02000000`.
[NanoBoyAdvance's DMA implementation](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/dma/dma.cc) and [ares DMA](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/gba/cpu/dma.cpp) use that boundary.
Both retain channel data across transfers and duplicate successful halfword reads.
These sources support the implemented subset, but do not establish cold hardware latch contents.

## Why CPU resume remains separate

[NanoBoyAdvance's bus](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/bus/bus.cc) tracks the last access type.
Its open-bus path can select DMA data when DMA was the preceding access.
DMA runs within CPU bus calls, so that condition depends on which CPU access resumes.
It does not imply that every first instruction after DMA must return a channel value.

[ares bus dispatch](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/gba/cpu/bus.cpp) routes DMA accesses through the relevant memory buses.
Its IWRAM bus and general data register are separate, and pending DMA runs before CPU accesses.
GBATEK's possible DMA influence on IWRAM is not sufficient to choose CPU resume ordering here.
Our instruction-boundary scheduler does not yet represent these per-access transitions.

Therefore, this change keeps successful-DMA invalidation of bounded Thumb IWRAM history.
Failed DMA preserves that history. No channel value is injected into CPU or BIOS snapshots.
The next CPU-bus work needs independent resume-ordering coverage rather than a universal last-transfer override.

## Implemented subset

Each `dma::Channel` holds `Option<u32>` retained data. `None` means synthetic startup has not established a value.
A pending `Transfer` includes that channel's value, but selection does not change it.
`Memory::step_dma` validates the destination and obtains either mapped source data or known blocked-source data.
For mapped halfword sources, both retained lanes receive the source halfword.
For blocked sources, the retained word stays unchanged and the destination selects the written halfword.
No BIOS image read occurs in the blocked path.

After a successful write and nominal device progress, completion commits the retained word with the unit's existing address/count changes.
Enable, disable, cancellation, completion, and repeat do not clear retained data.
Each channel remains independent. Host operations and CPU reads never seed channel data.
Unknown blocked sources retain `DmaError::UnsupportedSource`; known zero is valid data.
Other unmapped sources and unsupported controls remain diagnostics even with known channel data.

The existing atomic diagnostic policy commits no new latch value for a failed unit.
Earlier successful units remain visible. A partial failed read cannot replace their retained value.
Nominal source/destination costs, CPU pausing, trigger scheduling, and completion IRQs are unchanged.
General DMA open-bus reads, write-only register readback, local IWRAM lane effects, and CPU resume ownership remain outside this subset.

## Validation scope

Original regressions cover both widths, all channels, source/destination halfword lanes, and supplied or absent BIOS images.
They check independent ownership, zero/unknown values, host isolation, source-counter crossings, cancellation, repeats, priority, and failed-unit retention.
A CPU instruction sequence also checks that blocked DMA leaves the separately retained BIOS word unchanged.
The new retained-data cases failed on the previous unconditional blocked-source diagnostic.

External source and license files were reviewed outside the repository.
No external test ROM or source was imported. The hardware latch ROM was not built or run here.
These are original regression checks, not a local physical-hardware or external-emulator differential result.

Workspace and core/demo tests pass on Darwin arm64 in debug and release builds.
Formatting, lint checks, rustdoc, preparation tests, native ROM windows, and all graphics smoke modes pass.
Public ARM, Thumb, memory, and BIOS reports match the previous passing reports exactly.
Those suites are regression checks, not independent validation of retained DMA data or CPU bus handoff.
