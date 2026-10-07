# Audio device subset

The core models Direct Sound A/B, bounded sound DMA, and programmable sound generator (PSG) channels 1–4.
Wave channel 3 supports single-bank playback only. Optional fixed-rate capture supplies samples to the separate Darwin output adapter.
`Memory::audio_level()` exposes the current digital stereo level for deterministic inspection, not a continuous sample stream.

## Registers and PSG state

| Address | Behavior |
| --- | --- |
| `0x04000060..0x04000065` | Pulse channel 1 sweep, duty, length, envelope, frequency, and trigger |
| `0x04000068..0x04000069` | Pulse channel 2 duty, length, and envelope |
| `0x0400006c..0x0400006d` | Pulse channel 2 frequency and trigger; no sweep unit |
| `0x04000070..0x04000075` | Wave channel 3 gate, bank, length, volume, frequency, and trigger |
| `0x04000078..0x04000079` | Noise channel 4 length and envelope |
| `0x0400007c..0x0400007d` | Noise divider, counter width, shift, length enable, and trigger |
| `0x04000080..0x04000081` | PSG stereo routing and volume |
| `0x04000082` (`SOUNDCNT_H`) | Mixing, routing, timer selection, and FIFO reset strobes; readback mask `0x770f` |
| `0x04000084` (`SOUNDCNT_X`) | Master enable in bit 7; channel 1–4 activity in read-only bits 0–3 |
| `0x04000088` (`SOUNDBIAS`) | Bias/resolution configuration with mask `0xc3fe` |
| `0x04000090..0x0400009f` | CPU wave RAM window, opposite the bank selected by `SOUND3CNT_L` bit 6 |
| `0x040000a0..0x040000a7` | FIFO A/B writes with byte, halfword, and word access boundaries preserved |

Unused mapped register bytes read zero and ignore writes.
The halfwords at `0x04000066`, `0x0400006a`, and `0x0400006e` do not alias pulse registers or enable channel 2 sweep.
FIFO reads and the `0x0400008c..0x0400008f` gap remain explicit unmapped-access errors; no FIFO open-bus value is invented.
Raw memory starts with zero sound state. RegisterRamReset bit 6 sets the functional bias default to `0x0200`.

Master disable clears PSG configuration and the FIFO queues, but retains wave RAM and Direct Sound routing/bias.
The CPU then accesses wave bank 1. Idle bank selection can expose bank 0 after master enable.
The original BIOS reset clears bank 1 and both FIFO queues. It retains bank 0 and any separate in-flight playback word.
This is a functional reset subset, not a claim of complete firmware ordering or sound-reset equivalence.

Unsupported two-bank wave playback and active wave-bank changes return `MemoryError::UnsupportedIo` with the exact address and value.
All PSG writes are ignored while master sound is disabled.
Reserved PSG volume selection 3 is rejected while master sound is enabled, including enable after disabled configuration.

## Pulse channels 1 and 2

Each pulse channel has four duty patterns: 12.5%, 25%, 50%, and 75%.
Its eight-position waveform advances every `16 * (2048 - frequency)` system clocks.
The channels keep independent frequency, duty position, length, envelope, logical DAC gate, and activity state.
Only channel 1 has sweep. Channel 2 reuses the pulse engine without any mapped sweep register.
No general-purpose timer must be enabled for PSG playback.
Frequency writes change the next oscillator reload, not the remaining current interval.
A trigger reloads that channel's oscillator timer, envelope volume, and envelope timer.
Channel 1 also initializes its sweep shadow/timer state.
Retrigger preserves that channel's duty position and does not restart the other channel.
Master disable resets both pulse channels.

The modulation sequencer receives a 512 Hz clock, once per 32768 system clocks:

| Sequencer step | Event |
| --- | --- |
| 0, 2, 4, 6 | Length counter clock: 256 Hz |
| 2, 6 | Sweep clock: 128 Hz |
| 7 | Envelope clock: 64 Hz |

The divider starts at synthetic phase zero and continues while master sound is disabled.
Master enable sets the next sequencer step to zero without resetting that divider.
Neither channel trigger resets the shared sequencer. Both receive length/envelope clocks at the same sequencer phase.
HALT continues these clocks; STOP freezes them.
Waveform advancement uses arithmetic batches between sequencer events, not a loop for every system clock.

Length writes load `64 - length_field`. Enabled length expiry clears the channel activity flag.
Enabling length before a sequencer step that does not clock length applies an extra decrement.
A trigger reloads an empty counter to 64, or 63 when that extra-clock condition applies.
Nonempty length counters retain their values on trigger.

The envelope changes volume by one after each selected number of 64 Hz clocks.
Period zero holds volume. Saturation at zero or fifteen stops envelope changes without clearing channel activity.
Live envelope writes retain current volume and the remaining active countdown.
Reactivating a stopped envelope loads its new countdown. Trigger loads its configured initial volume.
Clearing envelope bits 3–7 disables the channel; setting them again requires a trigger to restart it.
This is the logical gate often called DAC enable in Game Boy references, not a separate physical GBA DAC.

Channel 1 sweep uses a shadow frequency independent of later frequency-register writes.
A nonzero shift checks overflow immediately at trigger.
Timed sweep updates check overflow both before and after committing the new frequency.
A result above 2047 disables the channel. Period zero suppresses periodic frequency changes, not the trigger-time check.
Shift zero performs timed overflow checks without committing a changed frequency.
Clearing the subtraction direction after a subtraction calculation disables the channel until a new trigger.

These rules define a nominal digital model, not exact hardware edge timing.
First-trigger output suppression, trigger sub-divider alignment, envelope trigger-edge delays, and revision-specific envelope write effects are not modeled.
The model samples the selected duty position immediately after trigger and uses a full-period timer reload.
The consulted implementations differ on some initial phases and envelope/sweep edge cases; no GBA hardware audio recording was used as an oracle.

## Noise channel 4

Noise uses a deterministic linear-feedback shift register (LFSR), not a host random-number generator.
The nominal counter follows GBATEK's GBA Galois description:

| Width | Trigger seed | Feedback mask | Sequence period |
| --- | --- | --- | --- |
| 15-bit | `0x4000` | `0x6000` | 32767 shifts |
| 7-bit | `0x0040` | `0x0060` | 127 shifts |

At each edge, the old low bit selects the signed output level.
The counter shifts right and, when that bit was one, XORs the feedback mask.
The period between edges is `(ratio == 0 ? 32 : 64 * ratio) << shift` system clocks.
All eight divider fields and sixteen shift fields use this GBA formula, including shifts 14 and 15.
No general-purpose timer or IRQ enable is required.

A trigger reloads the selected seed, full edge interval, envelope, and applicable length state.
The nominal held output starts low and changes at counter edges.
Live divider/shift writes preserve the remaining interval; the new period applies on the next reload.
Live width writes preserve the full counter, including transient upper bits, and select the new feedback mask.
They do not truncate or reseed the counter.
Exact hardware startup alignment and live-width transition behavior remain unverified.
The consulted mGBA implementation uses a different counter representation; this model does not claim matching initial sample phases.

Length, envelope, logical DAC gating, and activity use the same tested modulation implementation as the pulse channels.
All three channels receive the shared sequencer clocks but keep independent state.
Noise status uses SOUNDCNT_X bit 3. Envelope saturation alone does not clear it.
Master disable resets noise configuration and counter state. Muting its routing does not stop it.
The unused halfwords at `0x0400007a` and `0x0400007e` read zero and do not alias noise controls.

Large clock advances combine precomputed powers of the counter transition instead of shifting once per noise sample.
An independent bit-array reference checks these jump-ahead operations, including live-width transients and the final output carry.
Both complete sequence periods and maximum clock batches have deterministic regressions.
This is device-state advancement, not a sampled audio stream or hardware recording comparison.

## Wave channel 3: single-bank playback

The selected bank supplies 32 four-bit samples. The CPU reads and writes the opposite bank, including during playback.
SOUND3CNT_L retains gate, bank, and dimension bits with mask `0xe0`.
The volume register retains bits 13–15. Frequency, length-load, and trigger fields are write-only.
The unused halfword at `0x04000076` reads zero and ignores writes.

Each nominal sample period is `8 * (2048 - frequency)` system clocks.
Playback consumes each byte's high nibble before its low nibble, then proceeds to the next byte.
The selected bank rotates by one nibble per sample; it is not immutable storage addressed by a hidden pointer.
Stopping playback and exposing that bank therefore shows rotated data.
Complete 32-sample rotations restore the bank layout but still update the held output sample.
Clock batches compute the final rotation and sample directly, without looping once per sample.

A trigger reloads a full sample period and clears the held output to nominal silence until the first edge.
It cannot restore data already rotated in wave RAM. Retrigger uses the bank's current layout.
Frequency writes affect the next reload, not the remaining sample interval.
Clearing the gate stops activity. Setting the gate again requires a trigger to resume.
A trigger with the gate clear is accepted but cannot activate playback, including with an idle two-bank configuration.
Mute volume and disabled routes do not stop playback or clear activity.
Master disable resets channel configuration/state but preserves both wave RAM banks in their current layouts.

The length counter loads `256 - length_field` and uses the shared 256 Hz length clocks.
It follows the same nominal extra-clock/reload rule as other PSG channels, with maximum length 256 rather than 64.
A nonempty counter survives retrigger. Enabled expiry clears SOUNDCNT_X bit 2.
HALT continues sample and length clocks; STOP freezes them. No timer or IRQ enable is required.

The centered full-volume amplitude is `2 * (sample - 8)`, from -16 through +14.
Volume selects mute, 100%, 50%, or 25%. Force-volume overrides all selections with 75%.
The mixer retains quarter-units until after combining PSG sources, stereo volume, and the overall PSG ratio.
This avoids separately rounding fractional wave amplitudes. It follows the signed NanoBoyAdvance mixer comparison, not an analog recording.

### Evidence limits

Only dimension zero playback is implemented. Idle dimension-one configuration is writable, but triggering it with the gate enabled is diagnostic.
Selecting dimension one or changing the selected bank during active playback also remains diagnostic; clear the gate first.
These failures retain register lanes, RAM rotation, activity, and clocks through ordinary audio transactions.

GBATEK describes rotating wave RAM and selected-bank-first two-bank playback.
The pinned mGBA implementation rotates storage, while NanoBoyAdvance uses a sample pointer and changes bank at wrap.
Their two-bank starts, observable bank state, and retrigger behavior differ.
This increment does not choose an unverified two-bank model merely to accept more register writes.
The single-bank order/rate follows GBATEK; trigger phase, held-sample startup, live frequency/volume edges, and length quirks remain nominal.
There is no Game Boy wave-corruption model or physical GBA waveform validation.

Original tests cover every frequency and both single-bank selections, independent nibble rotation, complete rotations, maximum batches, and volume settings.
Integration checks cover masks, gating, status, stereo mixing/clipping, length, HALT/STOP, master reset, and BIOS reset.
ARM/Thumb/DMA tests verify trigger bus phases, status at length expiry, rotated RAM readback, and failed-step/block-store isolation.
Original terminal and native-window programs activate channel 3 without external game assets or host audio.

## FIFO and timer behavior

Each channel has a seven-word queue, a separate four-byte playback word, and a held signed sample.
Timer 0 or timer 1 overflow clocks the selected channel while master sound is enabled.
Timer 1 cascade overflows work. Timer IRQ enable and stereo routing do not gate sample consumption.
Timers 2 and 3 cannot clock Direct Sound.

On each selected overflow:

1. Request sound DMA if the queue contains at most three words, before loading a playback word.
2. Load a queued word if the playback word is empty.
3. Latch the next signed eight-bit sample in little-endian order.

The held sample remains unchanged between overflows. An empty playback word supplies zero on the next overflow.
This underflow policy follows both consulted emulator implementations; the cited hardware report leaves it uncertain.
A bulk advance consumes all available samples and then reaches zero without looping over every empty overflow.

A partial FIFO write updates the addressed lanes of the next queue slot and enqueues that whole word.
Other lanes retain that slot's previous bytes. A write to a full queue clears the queue.
Reset strobes clear queues without changing the held sample or remaining playback bytes.
Queue clearing also zeros stored queue slots in this nominal model.
Exact stale-lane behavior across overflow/reset remains unverified on hardware.

HALT continues timer/audio progress. STOP freezes it.
Master disable stops sample consumption; route disable only mutes the mixer path.

## Sound DMA

Special timing supports DMA1 to FIFO A and DMA2 to FIFO B.
Each request transfers four 32-bit words with a fixed destination, regardless of programmed count, width, or destination mode.
Source address control, channel priority, repeat enable, completion IRQ, and retained DMA data still apply.
Without repeat, the channel disables after the fourth word. With repeat, it waits for another timer request.

Other channel/FIFO pairings and non-FIFO destinations remain explicit `DmaError::UnsupportedControl` cases.
The references disagree about request routing outside the supported pairing; hardware reports permit non-FIFO destinations.
The core does not silently guess those configurations. Video capture and Game Pak DRQ remain unsupported.

Requests become visible at successful machine-step boundaries. Active DMA blocks ignore further requests.
Device-only bulk advances coalesce unserviced requests; they do not execute or replay DMA blocks.
Sub-instruction DMA arbitration and exact refill latency are not modeled.

## Mixing and timing ownership

Each routed Direct Sound sample contributes `sample * 2` at 50% volume or `sample * 4` at 100% volume.
Each active pulse or noise channel contributes a centered signed amplitude, `+volume` or `-volume`.
`SOUNDCNT_L` routes channel 1 through bits 8/12, channel 2 through bits 9/13, wave through bits 10/14, and noise through bits 11/15.
Each pair selects right/left respectively.
The mixer sums routed PSG amplitudes on each side, then multiplies by that side's volume field plus one.
`SOUNDCNT_H` applies the PSG ratio: 25%, 50%, or 100%, using one arithmetic right shift after summation.
Rounding each PSG channel separately would produce different low-volume levels and is not used.
At full stereo volume, each pulse/noise channel spans `-120..120`; wave spans `-128..112`, before mixing with Direct Sound.
This centered mixer follows GBATEK's signed range and the NanoBoyAdvance comparison; exact analog offset is not modeled.
The mixer adds all supported channels and the ten-bit bias, clips each side to `0..1023`, and subtracts 512.
`StereoLevel` therefore contains signed levels in `-512..511`. Master disable returns zero.
Bias resolution bits retain their values but do not yet drive pulse-width modulation (PWM) sampling or quantization.
The optional capture stream below samples this mixer. Hardware PWM sampling and analog filtering remain unimplemented.
Polling `audio_level()` once per video frame will not reconstruct audio.

Timer and sound state share the staged CPU/DMA clock.
Reads and writes observe their nominal bus-completion phase, after earlier timer overflows.
Successful steps commit once. Failed steps discard FIFO consumption, held samples, PSG clocks/state, register writes, and refill requests.
Block-store validation simulates sound enable/disable in a temporary state before committing any RAM or I/O writes.
Other devices retain their existing scheduling limits.

## Fixed-rate digital capture

`Memory::set_audio_capture(true)` enables optional stereo capture at `AUDIO_SAMPLE_RATE` (32,768 Hz).
The first sample occurs after 512 system clocks. Later samples use the same interval, independent of video frames.
This is a nominal digital stream, not the hardware PWM/resolution model selected by SOUNDBIAS.

Each sample observes PSG advancement and timer-driven Direct Sound consumption through its clock boundary.
When a CPU write completes on that same boundary, sampling precedes the write under the existing bus-phase ordering.
No host time, window code, device dependency, resampling, or file operation enters the core.
Master sound disable produces silence while capture continues. HALT continues capture; STOP freezes its phase and produces no frames.

`drain_audio_samples` copies committed stereo frames, oldest first, into a caller-provided slice.
Values use the same signed ten-bit scale as `StereoLevel`, not normalized host PCM.
CPU/DMA steps stage samples alongside timer/audio state. A failed step discards its samples and sample-clock changes.
Block-store preflight never publishes samples. Row capture does not clock sound a second time.
CPU-only stepping retains its existing no-device-clock behavior.

The committed queue holds at most `AUDIO_QUEUE_CAPACITY` (4,096) stereo frames.
A transaction stages at most eight frames. Ordinary instruction/DMA timing fits within this budget.
Full buffers discard new frames, retain older frames, and increment `audio_dropped_samples` without blocking emulation.
Large device-only clock advances bound work by available capacity, then advance remaining hardware state in bulk.
Draining often avoids loss; callers must inspect the drop counter rather than assuming lossless capture.

Capture is disabled by default. Changing enable state clears queued frames, phase, and the drop counter, but does not reset audio hardware.
Repeating the current enable state preserves capture state. Configure capture between machine steps.
Original tests cover exact sample boundaries, timer consumption, pulse phases, batching, queue overflow, HALT/STOP, and transactional rollback.

## Darwin output adapter

Use `--audio` with `--rom ... --window` at 1× speed to enable CPAL 0.16/CoreAudio output on macOS.
Windows remain muted by default. Terminal runs, ROM suites, and demos do not open an audio device.
ROM-window `--speed` values above 1 override `--audio`: no device is opened and sample capture stays disabled.
Sound hardware still executes at the same emulated cycle rates. Accelerated audio conversion is not implemented.
The flag is invalid in terminal mode and cannot be repeated. Other platforms currently report an explicit unsupported-platform error.

The adapter opens the default output device at its default configuration.
Supported formats are float32, signed16, and unsigned16, with 8–192 kHz rates and 1–32 channels.
Stereo maps to the first two channels. Mono averages left/right; additional channels receive silence.
Device initialization, unsupported formats, and stream errors fail the audio-enabled run rather than silently switching to muted execution.
Omit `--audio` to run without an audio device.

The core's 32,768 Hz frames enter a bounded 8,192-frame host queue.
Playback waits for 1,024 frames before starting or resuming after an underrun.
Linear interpolation converts to the device rate. A 20 Hz high-pass filter removes DC, followed by conservative 25% host gain and clipping.
These are presentation choices, not a measured GBA analog model or band-limited reconstruction.
High-frequency aliases, startup transients, and channel-model inaccuracies remain possible.

The data callback performs no allocation, blocking lock, emulation step, or log write.
It tries the queue lock once; contention produces silence and increments a counter.
An underrun produces silence and returns to the prefill state. Overflow discards new input frames and records the loss.
The final report includes submitted/nonzero-input frames, callbacks, nonzero-output frames, underruns, dropped input, and lock misses.
Zero submitted frames can indicate an unfocused window, not a failed output device.
The window treats a core capture overflow as an error; it drains samples between bounded CPU slices.
The callback cannot advance emulator clocks or request extra emulation work.

Focus loss and STOP clear queued host audio and conversion state. Muted frames are drained without being submitted.
The device may still finish its already-submitted buffer. Resume waits for fresh prefill instead of replaying stale samples.
Audio-enabled windows use absolute frame deadlines with at most one frame of catch-up.
This reduces sleep drift without skipping emulated frames or accumulating unbounded catch-up work.
Slow hosts can still underrun. Sample delivery is not a claim of uninterrupted or hardware-faithful sound.

Native checks on Darwin arm64 exercised a quiet original tone and an original wave-ROM window through CoreAudio.
A 900-frame local Emerald run reached its limit with 554,484 nonzero device-output frames, zero dropped input frames, and 13 underruns.
Those counters demonstrate output submission, not human listening, correct music, or game compatibility.
Audio quality and physical speaker/headphone output still require a manual listening check.

## Validation and remaining work

Original tests cover signed samples, byte order, queue capacity, partial writes, resets, underflow, stereo routing, and clipping.
They also cover cascaded timers, DMA block/repeat behavior, source progress, completion IRQs, HALT/STOP, and clock batching.
CPU/DMA failure tests verify transactional rollback and sound writes at bus completion.
BIOS tests cover disabled FIFO reset and the supported wave-bank policy.
Pulse tests cover every frequency/duty pair, waveform batching, status, length, envelope, sweep, mixing, and reserved-volume diagnostics.
They also check bus-completion status reads, retrigger timing, clock ownership, and rollback with both pulse channels running.
Channel 2 tests cover register gaps, absence of sweep, independent state, shared sequencer phase, and sum-before-rounding mixing.
Original CPU/DMA tests verify channel 2 activation and rollback when a later block-store value is unsupported.
Noise tests cover both counter widths, divider/shift fields, complete sequences, jump-ahead arithmetic, retrigger, and live rate/width writes.
They also cover modulation, independent status, stereo mixing, HALT/STOP, and scanline-capture independence.
CPU/DMA tests include noise register writes at bus completion and failed-step rollback with a running counter.
BIOS reset tests verify that selected sound reset stops all supported PSG channels, while unselected sound state remains active.

These tests validate the documented nominal model, not hardware audio fidelity.
Remaining audio work: two-bank wave playback, verified live bank changes, hardware PWM sampling, improved reconstruction, and lower-underrun pacing.
A local Emerald input probe exposed a wave trigger with the gate disabled. This is now accepted without activating playback.
The same scheduled-input probe passes that point and completes its extended frame budget; gameplay remains unverified.
Later native output checks exercise the host audio path separately; they do not establish game-audio accuracy.
See [the local runtime result](../research/emerald-reset.md).

## References

- [CPAL 0.16.0](https://docs.rs/cpal/0.16.0/cpal/): default device/configuration, typed output callbacks, sample formats, CoreAudio backend, and stream lifecycle. CPAL is a macOS-only desktop dependency here.

- [GBATEK wave channel 3](https://problemkaputt.de/gbatek-gba-sound-channel-3-wave-output.htm): bank selection, nibble order, rotating RAM, sample rate, length, and force volume.
- [NanoBoyAdvance wave at 55b5cf0a](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/apu/channel/wave_channel.cc) and [header](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/apu/channel/wave_channel.hh): signed gain and rate comparison; pointer-based RAM and two-bank/retrigger differences are not copied.

- [GBATEK noise channel 4](https://mgba-emu.github.io/gbatek/): length/envelope registers, divider formula, Galois counter seeds, masks, and sequence periods.
- [NanoBoyAdvance noise at 55b5cf0a](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/apu/channel/noise_channel.cc) and [noise header](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/apu/channel/noise_channel.hh): GBA divider scaling, counter feedback, trigger, and register behavior comparison.
- [GBATEK channels 1 and 2](https://mgba-emu.github.io/gbatek/): register fields, duty ratios, modulation rates, channel 2's register gap, and absence of sweep.
- [Pan Docs audio details](https://gbdev.io/pandocs/Audio_details.html): duty counters, shadow sweep, length edge rules, and GBA digital-mixer differences. GB-only edge details are not treated as verified GBA measurements.
- [GbdevWiki sound hardware](https://gbdev.gg8.se/wiki/articles/Gameboy_sound_hardware): background sequencer and sweep descriptions.
- [mGBA shared PSG at 3a5e34be](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gb/audio.c): GBA-style sequencer, length, envelope, sweep, and waveform comparison.
- [NanoBoyAdvance pulse at 55b5cf0a](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/apu/channel/quad_channel.cc), [base channel](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/apu/channel/base_channel.hh), [length](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/apu/channel/length_counter.hh), [envelope](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/apu/channel/envelope.hh), and [sweep](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/apu/channel/sweep.hh): ordinary pulse behavior and documented uncertainty. The adjacent `quad_channel.hh` defines frequency scaling.
- [GBATEK Direct Sound](https://problemkaputt.de/gbatek-gba-sound-channel-a-and-b-dma-sound.htm): registers, timer selection, signed samples, and DMA blocks.
- [GBATEK sound control](https://problemkaputt.de/gbatek-gba-sound-control-registers.htm): master enable, read masks, reset strobes, routing, and bias.
- [GBATEK mirror](https://mgba-emu.github.io/gbatek/): wave-bank access and register layout.
- [gbadev Direct Sound overview](https://gbadev.net/gbadoc/audio/directsound.html): introductory FIFO/timer/DMA behavior; not authority for queue edge cases.
- [Gericom's hardware findings, mGBA issue 1847](https://github.com/mgba-emu/mgba/issues/1847): seven-word queue, separate playback buffer, request order, partial writes, and overflow observations.
- [mGBA audio at 3a5e34be](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gba/audio.c): playback, reset, mixing, and destination-based request routing comparison.
- [NanoBoyAdvance APU at 55b5cf0a](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/apu/apu.cc): master gating, playback word, underflow, and mixing.
- [NanoBoyAdvance FIFO](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/apu/channel/fifo.hh), [registers](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/apu/registers.cc), and [DMA](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/dma/dma.cc): queue/reset policy and channel-based request routing comparison.
- [mGBA BIOS at 3a5e34be](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gba/bios.c): functional reset defaults.

The Rust implementation and regression programs are original. No upstream implementation or external test ROM was copied into the repository.
