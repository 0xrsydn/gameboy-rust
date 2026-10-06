# Instruction fetches across memory-region boundaries

## Finding

The shared sampler already selected bus width from the new fetch address.
However, `Memory::begin_cpu_access` rejected every Thumb sample whose upper address byte differed from the executing PC.
It also selected the IWRAM latch from the executing PC rather than the actual fetch address.
This prevented mapped boundary fetches from supplying an otherwise known bus value.

The boundary extension removes that blanket restriction.
Thumb unused-memory reads now use the bus observation at the newly fetched address P+4.
ARM continues to use its fetched PC+8 word.
This is a bounded entry-snapshot model, not a complete general-bus or cycle-accurate implementation.

## Evidence and limits

A boundary-focused web search revisited [nocash's open-bus discussion](https://www.ngemu.com/threads/gba-open-bus.170809/).
It describes repeated Thumb halfwords, full BIOS/OAM words, and history-dependent IWRAM lanes.
The search did not identify a dedicated physical-hardware boundary result for this implementation.
Those formulas alone do not establish every boundary or timing case.

The stronger implementation evidence is address-based bus dispatch:

- [ares bus at 6f6786e0](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/gba/cpu/bus.cpp) selects the bus using the actual access address.
  It assigns the full result to `mdr` before selecting the addressed CPU halfword/byte.
  It does not reject a mapped fetch because the executing instruction occupies another region.
- [ares memory helpers](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/gba/cpu/memory.cpp) return repeated halfwords from 16-bit regions and the complete local latch from IWRAM.
- [ares OAM helper](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/gba/ppu/memory.cpp) returns the aligned word before CPU halfword selection.
- [jgenesis bus at fab6e2cc](https://github.com/jsgroth/jgenesis/blob/fab6e2ccc60e492dd68b7f1e927b0829a6d80195/backend/gba-core/src/bus.rs) independently updates general open bus from access width and the addressed memory helper.
  Its `iwram_update_open_bus` drives the complete local IWRAM word after a partial-lane update.
  That revision marks OAM behavior as a TODO, so it is not independent confirmation of the full OAM rule.
- [NanoBoyAdvance's loop/refill helpers](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/arm/arm7tdmi.hh) place the new fetch before execution and refill target pairs in order.
  Its [open-bus helper](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/bus/bus.cc) uses opcode-based approximations with a BIOS/OAM TODO.
  We do not treat that helper as an exact boundary oracle.

These are source comparisons, not physical-hardware measurements or external-emulator differential runs.
No external code, BIOS image, or test ROM was imported. All regression programs are original.

## Supported behavior

The instruction buffer and bus history consume the same captured new fetch.
The consumer does not reread memory or replay observations from retained instructions.

| Lookahead transition | Thumb unused-memory snapshot |
| --- | --- |
| Palette to VRAM | Newly fetched halfword repeated in both lanes |
| VRAM to OAM | Full aligned OAM word |
| OAM to mapped ROM | Newly fetched ROM halfword repeated in both lanes |
| Mapped ROM page or wait-window boundary | Halfword from the normal mapped cartridge offset, repeated |
| EWRAM to IWRAM | Persistent IWRAM word after the new fetch drives its addressed halfword |

The existing ROM mapping applies across all three wait-state windows.
A 16-bit fetch needs only its two mapped bytes, not an adjacent full word.
This includes OAM-to-ROM crossings, despite the executing instruction still residing on a 32-bit bus.

IWRAM does not acquire guessed lanes from adjacent memory.
At P=`0x02fffffc`, only the low half has been fetched from IWRAM; the high half needs prior observed history.
Without that history, an unused-memory load remains diagnostic.
At P=`0x02fffffe`, a cold fill or target-pair refill can capture IWRAM's low half before the high lookahead fetch.
The resulting complete local word is then available.
A preceding CPU data access or successful DMA can also establish the required lanes.

A refill can span two regions. Each actual IWRAM target sample updates only its addressed lanes.
Later host writes do not replace those captured lanes; the target instruction's new lookahead updates its own lane normally.
Diagnostic failures discard staged fetch/history changes and preserve the previous instruction buffer.
A retry takes a new lookahead sample from current memory.

## Preserved diagnostics and timing

- Missing ROM bytes and unmapped instruction reads remain strict errors or deferred fetch diagnostics.
- Unsupported I/O fetch observations do not borrow known IWRAM lanes or another region's bus value.
- Unknown IWRAM lanes do not become zero or fabricated history.
- An ordinary instruction or mapped data load does not require a complete unused-memory snapshot.
- A branch can discard an unavailable sequential path without failing early.
- Host reads, instruction fetches, and DMA do not inherit CPU unused-memory fallback.
- Protected BIOS history remains separate and image-derived.
- All data reads within an instruction still use one entry snapshot.

The later [fetch-address timing extension](fetch-address-timing.md) charges non-refill code at the actual lookahead address.
The [refill timing extension](refill-fetch-timing.md) also charges source fetches and target pairs individually.
The later [sequencing extension](fetch-access-sequencing.md) retains source kinds across CPU data/internal work.
Cold-start costs remain nominal.
Per-access device updates, video-bus contention, general DMA handoff, and Game Pak prefetch remain incomplete.
No boundary timing accuracy is claimed.

## Original regression coverage

Tests cover both Thumb alignments and cold/retained pipelines at mapped video/ROM boundaries.
Load matrices check byte, halfword, word, signed and unaligned results without flag changes.
Other tests cover block/stack writeback, CPU/DMA-seeded IWRAM lanes, unknown lanes, split refills, and failed-step retry.
ROM tests span mapped page and wait-window boundaries with distinct wait settings.
Short-ROM tests require exactly the fetched halfword.
Unsupported I/O, missing ROM, discarded branch paths, strict host reads, and unchanged ARM snapshots remain covered.

The mapped-crossing regressions failed under the old upper-address-byte guard, then passed with fetch-address selection.

## Validation result

Workspace and core/demo tests pass in debug and release on Darwin arm64.
Formatting, lint checks, rustdoc, preparation tests, native ROM windows, and graphics smoke modes pass.
Public ARM, Thumb, memory, and BIOS reports match the prior persistent-IWRAM reports exactly, including nominal cycle counts.
Debug and release reports also match for the same fixture paths.
These checks are regression results, not physical-hardware boundary or timing conformance tests.
