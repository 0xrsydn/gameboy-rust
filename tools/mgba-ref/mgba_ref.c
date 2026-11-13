// mgba-ref: drive mGBA's core library as a reference emulator over stdin/stdout.
//
// Original code for this repository. It links against a locally built libmgba
// (MPL-2.0, https://mgba.io), which is never committed here; see
// docs/mgba-reference.md. Build it with tools/prepare_mgba_reference.py.
//
// Protocol: one command per line on stdin. Every reply starts with a text line,
// "ok ..." or "err <message>". Replies to `read` and `fb` follow their header
// with exactly the announced number of raw bytes.
//
//   load ROM [BIOS]   load a ROM, optionally with a BIOS image, and reset
//   keys MASK         hold buttons (GBA KEYINPUT bit order, pressed-high)
//   frame             run to the next frame boundary; reply "ok FRAMECOUNTER"
//   step              execute one CPU instruction
//   regs              reply "ok r0 .. r15 cpsr spsr mode" in hex
//   read ADDR LEN     reply "ok LEN" then LEN raw bytes (side-effect-free view)
//   fb                reply "ok 240 160" then 240*160 little-endian u32 pixels
//   write16 ADDR VAL  store a halfword through the CPU bus (for aligning start state)
//   video LINE CYCLE  move the display to LINE at CYCLE, before any frame runs
//   sync COUNT        run COUNT instructions outside the BIOS (0x0000-0x3fff); reply
//                     "ok COUNT", then per instruction one line:
//                     "ADDR THUMB CYCLES r0 .. r15 CPSR" (hex, CYCLES decimal), with
//                     registers and the global cycle count right after it executed.
//                     An instruction that enters the BIOS (SWI) is reported once
//                     control leaves the BIOS again.
//   quit

#include <mgba/core/core.h>
#include <mgba/core/config.h>
#include <mgba/core/log.h>
#include <mgba/internal/arm/arm.h>
#include <mgba/internal/gba/gba.h>
#include <mgba/internal/gba/io.h>
#include <mgba-util/vfs.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define WIDTH 240
#define HEIGHT 160

static struct mCore* core;
static color_t framebuffer[WIDTH * HEIGHT];

// mGBA keeps globalCycles only in debugger builds. masterCycles plus the CPU's
// pending cycles is maintained in every build; extend it to 64 bits here.
static uint64_t clock_total;
static uint32_t clock_last;

static void silent_log(struct mLogger* logger, int category, enum mLogLevel level, const char* format, va_list args) {
	(void) logger;
	(void) category;
	(void) level;
	(void) format;
	(void) args;
}

static struct mLogger logger = { .log = silent_log };

static void reply_err(const char* message) {
	printf("err %s\n", message);
	fflush(stdout);
}

static void reply_ok(void) {
	printf("ok\n");
	fflush(stdout);
}

static int load(const char* rom, const char* bios) {
	if (core) {
		mCoreConfigDeinit(&core->config);
		core->deinit(core);
		core = NULL;
	}
	core = mCoreFind(rom);
	if (!core || core->platform(core) != mPLATFORM_GBA) {
		core = NULL;
		reply_err("not a GBA ROM");
		return 0;
	}
	core->init(core);
	mCoreInitConfig(core, NULL);
	// Without a BIOS image, start at the cartridge entry the way mGBA's skip-BIOS
	// path does (VCOUNT 126, POSTFLG 1), and use mGBA's high-level BIOS calls.
	core->opts.useBios = bios != NULL;
	core->opts.skipBios = bios == NULL;
	core->setVideoBuffer(core, framebuffer, WIDTH);
	if (!mCoreLoadFile(core, rom)) {
		reply_err("cannot load ROM");
		return 0;
	}
	if (bios) {
		struct VFile* vf = VFileOpen(bios, O_RDONLY);
		if (!vf || !core->loadBIOS(core, vf, 0)) {
			reply_err("cannot load BIOS");
			return 0;
		}
	}
	core->reset(core);
	// Cycle-accurate comparison: never skip known or detected idle loops.
	((struct GBA*) core->board)->idleOptimization = IDLE_LOOP_IGNORE;
	clock_total = 0;
	clock_last = (uint32_t) mTimingCurrentTime(&((struct GBA*) core->board)->timing);
	reply_ok();
	return 1;
}

static uint64_t now(void) {
	struct GBA* gba = core->board;
	uint32_t current = (uint32_t) mTimingCurrentTime(&gba->timing);
	clock_total += (uint32_t) (current - clock_last);
	clock_last = current;
	return clock_total;
}

static uint32_t exec_address(const struct ARMCore* cpu) {
	return (uint32_t) cpu->gprs[ARM_PC] - (cpu->executionMode == MODE_THUMB ? WORD_SIZE_THUMB : WORD_SIZE_ARM);
}

static void sync_user(unsigned long count) {
	struct GBA* gba = core->board;
	struct ARMCore* cpu = gba->cpu;
	printf("ok %lu\n", count);
	for (unsigned long record = 0; record < count; ++record) {
		for (unsigned long guard = 0;; ++guard) {
			if (guard > 100000000UL) {
				printf("stuck\n");
				fflush(stdout);
				return;
			}
			// Mirror ARMRun: pending events (IRQ entry, DMA, HALT) first, then one instruction.
			while (cpu->cycles >= cpu->nextEvent) {
				cpu->irqh.processEvents(cpu);
			}
			uint32_t address = exec_address(cpu);
			int thumb = cpu->executionMode == MODE_THUMB;
			ARMRun(cpu);
			if (address >= 0x4000) {
				// mGBA runs most BIOS calls inside the SWI; the rest enter BIOS code.
				for (unsigned long inside = 0; exec_address(cpu) < 0x4000; ++inside) {
					if (inside > 100000000UL) {
						printf("stuck\n");
						fflush(stdout);
						return;
					}
					while (cpu->cycles >= cpu->nextEvent) {
						cpu->irqh.processEvents(cpu);
					}
					if (exec_address(cpu) >= 0x4000) {
						break;
					}
					ARMRun(cpu);
				}
				printf("%08x %d %llu", address, thumb, (unsigned long long) now());
				for (int i = 0; i < 16; ++i) {
					printf(" %08x", (uint32_t) cpu->gprs[i]);
				}
				printf(" %08x\n", (uint32_t) cpu->cpsr.packed);
				break;
			}
		}
	}
	fflush(stdout);
}

static void regs(void) {
	struct ARMCore* cpu = ((struct GBA*) core->board)->cpu;
	printf("ok");
	for (int i = 0; i < 16; ++i) {
		printf(" %08x", (uint32_t) cpu->gprs[i]);
	}
	printf(" %08x %08x %s\n", (uint32_t) cpu->cpsr.packed, (uint32_t) cpu->spsr.packed,
		cpu->executionMode == MODE_THUMB ? "thumb" : "arm");
	fflush(stdout);
}

static void read_memory(uint32_t address, uint32_t length) {
	if (length > 0x01000000) {
		reply_err("length too large");
		return;
	}
	uint8_t* bytes = malloc(length ? length : 1);
	for (uint32_t i = 0; i < length; ++i) {
		bytes[i] = (uint8_t) core->rawRead8(core, address + i, -1);
	}
	printf("ok %u\n", length);
	fwrite(bytes, 1, length, stdout);
	fflush(stdout);
	free(bytes);
}

// Reposition the display clock the way mGBA's own skip-BIOS start does: the
// pending event is the next HBlank, VIDEO_HDRAW_LENGTH cycles into the line.
static void video(uint32_t line, uint32_t cycle) {
	struct GBA* gba = core->board;
	if (line >= VIDEO_VERTICAL_TOTAL_PIXELS || cycle >= VIDEO_HDRAW_LENGTH) {
		reply_err("line or cycle out of range");
		return;
	}
	gba->video.vcount = line;
	gba->memory.io[REG_VCOUNT >> 1] = line;
	mTimingDeschedule(&gba->timing, &gba->video.event);
	mTimingSchedule(&gba->timing, &gba->video.event, VIDEO_HDRAW_LENGTH - cycle);
	reply_ok();
}

static void frame_buffer(void) {
	printf("ok %d %d\n", WIDTH, HEIGHT);
	for (int i = 0; i < WIDTH * HEIGHT; ++i) {
		uint32_t pixel = (uint32_t) framebuffer[i];
		uint8_t le[4] = { pixel, pixel >> 8, pixel >> 16, pixel >> 24 };
		fwrite(le, 1, 4, stdout);
	}
	fflush(stdout);
}

int main(void) {
	mLogSetDefaultLogger(&logger);
	char line[4096];
	while (fgets(line, sizeof(line), stdin)) {
		line[strcspn(line, "\r\n")] = '\0';
		char* command = strtok(line, " ");
		if (!command) {
			continue;
		}
		if (!strcmp(command, "quit")) {
			break;
		}
		if (!strcmp(command, "load")) {
			char* rom = strtok(NULL, " ");
			char* bios = strtok(NULL, " ");
			if (!rom) {
				reply_err("usage: load ROM [BIOS]");
			} else {
				load(rom, bios);
			}
			continue;
		}
		if (!core) {
			reply_err("no ROM loaded");
			continue;
		}
		if (!strcmp(command, "keys")) {
			char* mask = strtok(NULL, " ");
			core->setKeys(core, mask ? (uint32_t) strtoul(mask, NULL, 0) : 0);
			reply_ok();
		} else if (!strcmp(command, "frame")) {
			core->runFrame(core);
			printf("ok %u\n", core->frameCounter(core));
			fflush(stdout);
		} else if (!strcmp(command, "step")) {
			core->step(core);
			reply_ok();
		} else if (!strcmp(command, "regs")) {
			regs();
		} else if (!strcmp(command, "read")) {
			char* address = strtok(NULL, " ");
			char* length = strtok(NULL, " ");
			if (!address || !length) {
				reply_err("usage: read ADDR LEN");
			} else {
				read_memory((uint32_t) strtoul(address, NULL, 0), (uint32_t) strtoul(length, NULL, 0));
			}
		} else if (!strcmp(command, "fb")) {
			frame_buffer();
		} else if (!strcmp(command, "write16")) {
			char* address = strtok(NULL, " ");
			char* value = strtok(NULL, " ");
			if (!address || !value) {
				reply_err("usage: write16 ADDR VALUE");
			} else {
				core->busWrite16(core, (uint32_t) strtoul(address, NULL, 0), (uint16_t) strtoul(value, NULL, 0));
				reply_ok();
			}
		} else if (!strcmp(command, "sync")) {
			char* count = strtok(NULL, " ");
			sync_user(count ? strtoul(count, NULL, 0) : 1);
		} else if (!strcmp(command, "video")) {
			char* line = strtok(NULL, " ");
			char* cycle = strtok(NULL, " ");
			if (!line || !cycle) {
				reply_err("usage: video LINE CYCLE");
			} else {
				video((uint32_t) strtoul(line, NULL, 0), (uint32_t) strtoul(cycle, NULL, 0));
			}
		} else {
			reply_err("unknown command");
		}
	}
	if (core) {
		mCoreConfigDeinit(&core->config);
		core->deinit(core);
	}
	return 0;
}
