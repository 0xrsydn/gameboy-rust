// mesen-ref: drive Mesen 2's GBA core as a second reference emulator over stdin/stdout.
//
// Original code for this repository. It links against a locally built Mesen 2 GBA core
// (GPL-3.0-only, https://github.com/SourMesen/Mesen2, through the single-threaded headless
// fork pinned in tools/mesen-reference.lock.json), which is never committed here. The linked
// binary is therefore GPL-3.0-only and stays local; see docs/mesen-reference.md. Build it with
// tools/prepare_mesen_reference.py.
//
// It speaks mgba-ref's protocol (tools/mgba-ref/mgba_ref.c), so gba-diff drives either one.
// One command per line on stdin. Every reply starts with a text line, "ok ..." or
// "err <message>". Replies to `read` and `fb` follow their header with exactly the announced
// number of raw bytes. Mesen's own log lines go to stderr, never into the reply stream.
//
//   name              reply "ok mesen2"; mgba-ref answers "err unknown command"
//   load ROM BIOS     load a ROM with a 16 KiB BIOS image, then power on at the reset vector.
//                     Mesen has no high-level BIOS, so the image is required.
//   keys MASK         hold buttons (GBA KEYINPUT bit order, pressed-high), effective at once
//   frame             run to the next VBlank; reply "ok FRAMECOUNTER"
//   step              run one CPU step (one instruction, or one halted cycle)
//   regs              reply "ok r0 .. r15 cpsr spsr mode" in hex; r15 is reported the way
//                     mgba-ref does, as the next instruction's address + 4 (ARM) or + 2 (Thumb)
//   read ADDR LEN     reply "ok LEN" then LEN raw bytes (Mesen's debugger view, no side effects)
//   fb                reply "ok 240 160" then 240*160 little-endian u32 pixels, each 5-bit
//                     channel as value << 3 with red in the low byte
//   sync COUNT        run COUNT instructions outside the BIOS (0x0000-0x3fff); reply
//                     "ok COUNT", then per instruction one line:
//                     "ADDR THUMB CYCLES r0 .. r15 CPSR" (hex, CYCLES decimal), with
//                     registers and the master clock right after it executed.
//                     An instruction that enters the BIOS (SWI, or an IRQ taken after it) is
//                     reported once control leaves the BIOS again.
//   quit
//
// mgba-ref's `write16`, `video`, and `rtc` exist to align mGBA's skip-BIOS start and clock.
// Mesen runs the same firmware from reset as this emulator, so they are not needed and
// answer "err unsupported by mesen-ref".

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <memory>
#include <string>
#include <vector>

#include <unistd.h>

#include "pch.h"
#include "Shared/Emulator.h"
#include "Shared/EmuSettings.h"
#include "Shared/SettingTypes.h"
#include "Shared/Interfaces/IConsole.h"
#include "Shared/Interfaces/IInputProvider.h"
#include "Shared/BaseControlDevice.h"
#include "Shared/BaseControlManager.h"
#include "GBA/GbaConsole.h"
#include "GBA/GbaCpu.h"
#include "GBA/GbaMemoryManager.h"
#include "GBA/GbaPpu.h"
#include "GBA/GbaTypes.h"
#include "GBA/Input/GbaController.h"
#include "Utilities/FolderUtilities.h"
#include "Utilities/VirtualFile.h"

namespace {

constexpr int WIDTH = 240;
constexpr int HEIGHT = 160;
constexpr uint32_t BIOS_SIZE = 0x4000;
constexpr uint32_t BIOS_END = 0x4000;
constexpr uint64_t CYCLES_PER_LINE = 1232;
constexpr unsigned long GUARD = 100000000UL;

FILE* reply = nullptr;
std::string home;
std::unique_ptr<Emulator> emulator;
uint16_t held = 0;

// KEYINPUT bit order (A, B, Select, Start, Right, Left, Up, Down, R, L) to Mesen's buttons.
const int BUTTONS[10] = {
	GbaController::A, GbaController::B, GbaController::Select, GbaController::Start,
	GbaController::Right, GbaController::Left, GbaController::Up, GbaController::Down,
	GbaController::R, GbaController::L,
};

struct HeldKeys : IInputProvider {
	bool SetInput(BaseControlDevice* device) override {
		if(device->GetControllerType() != ControllerType::GbaController) {
			return false;
		}
		device->ClearState();
		for(int bit = 0; bit < 10; ++bit) {
			if(held & (1 << bit)) {
				device->SetBit(BUTTONS[bit]);
			}
		}
		return true;
	}
};

HeldKeys keys_provider;

void reply_err(const char* message) {
	fprintf(reply, "err %s\n", message);
	fflush(reply);
}

void reply_ok() {
	fprintf(reply, "ok\n");
	fflush(reply);
}

GbaConsole* console() {
	return static_cast<GbaConsole*>(emulator->GetConsole().get());
}

void unload() {
	emulator.reset();
	if(!home.empty()) {
		std::error_code ignored;
		std::filesystem::remove_all(home, ignored);
		home.clear();
	}
}

bool read_file(const char* path, std::vector<uint8_t>& bytes) {
	FILE* file = fopen(path, "rb");
	if(!file) {
		return false;
	}
	uint8_t buffer[65536];
	size_t count;
	while((count = fread(buffer, 1, sizeof(buffer), file)) > 0) {
		bytes.insert(bytes.end(), buffer, buffer + count);
	}
	bool ok = !ferror(file);
	fclose(file);
	return ok;
}

void load(const char* rom, const char* bios) {
	unload();
	std::vector<uint8_t> rom_bytes;
	std::vector<uint8_t> bios_bytes;
	if(!read_file(rom, rom_bytes)) {
		reply_err("cannot read ROM");
		return;
	}
	if(!read_file(bios, bios_bytes) || bios_bytes.size() != BIOS_SIZE) {
		reply_err("BIOS image must be a readable 16 KiB file");
		return;
	}

	// A fresh home per load: Mesen reads the BIOS from <home>/Firmware/gba_bios.bin and keeps
	// battery saves under <home>/Saves, which must not carry over between runs.
	const char* temp = getenv("TMPDIR");
	std::string pattern = std::string(temp && *temp ? temp : "/tmp") + "/mesen-ref-XXXXXX";
	std::vector<char> name(pattern.begin(), pattern.end());
	name.push_back('\0');
	if(!mkdtemp(name.data())) {
		reply_err("cannot create a temporary home folder");
		return;
	}
	home = name.data();
	FolderUtilities::SetHomeFolder(home);
	std::filesystem::create_directories(home + "/Firmware");
	FILE* firmware = fopen((home + "/Firmware/gba_bios.bin").c_str(), "wb");
	if(!firmware || fwrite(bios_bytes.data(), 1, BIOS_SIZE, firmware) != BIOS_SIZE) {
		if(firmware) {
			fclose(firmware);
		}
		reply_err("cannot write the BIOS image");
		return;
	}
	fclose(firmware);

	emulator = std::make_unique<Emulator>();
	emulator->Initialize(false);
	GbaConfig& config = emulator->GetSettings()->GetGbaConfig();
	// Power on at the reset vector and run the supplied firmware, like this emulator.
	config.SkipBootScreen = false;
	// Render every frame; Mesen otherwise skips frames that finish within 15 ms of the last.
	config.DisableFrameSkipping = true;
	config.RamPowerOnState = RamState::AllZeros;
	// Mesen's RTC follows the host clock, which would make runs irreproducible.
	config.RtcType = GbaRtcType::Disabled;
	config.SaveType = GbaSaveType::AutoDetect;
	config.OverclockScanlineCount = 0;

	VirtualFile file(rom_bytes.data(), rom_bytes.size(), "rom.gba");
	if(!emulator->LoadRom(file, VirtualFile())) {
		unload();
		reply_err("Mesen could not load the ROM");
		return;
	}
	if(std::memcmp(emulator->GetMemory(MemoryType::GbaBootRom).Memory, bios_bytes.data(), BIOS_SIZE) != 0) {
		unload();
		reply_err("Mesen did not load the BIOS image");
		return;
	}
	// Mesen powers its display on at line 225, cycle 0 (upstream d473d0e, fitted to timing
	// tests run through the official BIOS); this emulator starts at line 0, cycle 0. Neither
	// is anchored to hardware with this firmware, so start Mesen where this emulator starts,
	// as gba-diff moves mGBA's display clock to its own.
	uint64_t clock = console()->GetMasterClock();
	if(clock >= CYCLES_PER_LINE) {
		unload();
		reply_err("Mesen ran past the first line during power-on");
		return;
	}
	GbaPpuState& ppu = console()->GetPpu()->GetState();
	ppu.Scanline = 0;
	ppu.Cycle = (uint16_t)clock;
	emulator->GetConsole()->GetControlManager()->RegisterInputProvider(&keys_provider);
	held = 0;
	reply_ok();
}

void set_keys(uint16_t mask) {
	held = mask & 0x3ff;
	// Mesen polls input once per frame, at VBlank. Poll now so a change applies at once, as in
	// mgba-ref and this emulator.
	console()->GetControlManager()->UpdateInputState();
	reply_ok();
}

GbaCpuState& cpu_state() {
	return console()->GetCpu()->GetState();
}

// Execute one CPU step. Returns false for a halted cycle that ran no instruction.
bool exec_one() {
	GbaCpu* cpu = console()->GetCpu();
	bool halted = cpu->IsHalted();
	cpu->Exec<true, false>();
	return !(halted && cpu->IsHalted());
}

// The address of the next instruction to execute.
uint32_t next_address() {
	return cpu_state().Pipeline.Execute.Address;
}

// mgba-ref's r15: the next instruction's address plus one instruction width.
uint32_t reported_pc() {
	GbaCpuState& state = cpu_state();
	return state.Pipeline.Execute.Address + (state.CPSR.Thumb ? 2 : 4);
}

uint32_t spsr(GbaCpuState& state) {
	switch(state.CPSR.Mode) {
		case GbaCpuMode::Fiq: return state.FiqSpsr.ToInt32();
		case GbaCpuMode::Irq: return state.IrqSpsr.ToInt32();
		case GbaCpuMode::Supervisor: return state.SupervisorSpsr.ToInt32();
		case GbaCpuMode::Abort: return state.AbortSpsr.ToInt32();
		case GbaCpuMode::Undefined: return state.UndefinedSpsr.ToInt32();
		default: return 0;
	}
}

void print_registers(GbaCpuState& state) {
	for(int i = 0; i < 15; ++i) {
		fprintf(reply, " %08x", state.R[i]);
	}
	fprintf(reply, " %08x", reported_pc());
}

void regs() {
	GbaCpuState& state = cpu_state();
	fprintf(reply, "ok");
	print_registers(state);
	fprintf(reply, " %08x %08x %s\n", state.CPSR.ToInt32(), spsr(state), state.CPSR.Thumb ? "thumb" : "arm");
	fflush(reply);
}

void sync_user(unsigned long count) {
	fprintf(reply, "ok %lu\n", count);
	for(unsigned long record = 0; record < count; ++record) {
		for(unsigned long guard = 0;; ++guard) {
			if(guard > GUARD) {
				fprintf(reply, "stuck\n");
				fflush(reply);
				return;
			}
			uint32_t address = next_address();
			bool thumb = cpu_state().CPSR.Thumb;
			if(!exec_one() || address < BIOS_END) {
				continue;
			}
			for(unsigned long inside = 0; next_address() < BIOS_END; ++inside) {
				if(inside > GUARD) {
					fprintf(reply, "stuck\n");
					fflush(reply);
					return;
				}
				exec_one();
			}
			GbaCpuState& state = cpu_state();
			fprintf(reply, "%08x %d %llu", address, thumb ? 1 : 0,
				(unsigned long long)console()->GetMasterClock());
			print_registers(state);
			fprintf(reply, " %08x\n", state.CPSR.ToInt32());
			break;
		}
	}
	fflush(reply);
}

void read_memory(uint32_t address, uint32_t length) {
	if(length > 0x01000000) {
		reply_err("length too large");
		return;
	}
	GbaMemoryManager* memory = console()->GetMemoryManager();
	std::vector<uint8_t> bytes(length);
	for(uint32_t i = 0; i < length; ++i) {
		bytes[i] = memory->DebugRead(address + i);
	}
	fprintf(reply, "ok %u\n", length);
	fwrite(bytes.data(), 1, length, reply);
	fflush(reply);
}

void frame_buffer() {
	// The buffer the PPU finished at the last VBlank; it swaps at the next line 0.
	const uint16_t* pixels = console()->GetPpu()->GetScreenBuffer();
	fprintf(reply, "ok %d %d\n", WIDTH, HEIGHT);
	for(int i = 0; i < WIDTH * HEIGHT; ++i) {
		uint32_t c = pixels[i];
		uint32_t pixel = (c & 0x1f) << 3 | ((c >> 5) & 0x1f) << 11 | ((c >> 10) & 0x1f) << 19;
		uint8_t le[4] = { (uint8_t)pixel, (uint8_t)(pixel >> 8), (uint8_t)(pixel >> 16), (uint8_t)(pixel >> 24) };
		fwrite(le, 1, 4, reply);
	}
	fflush(reply);
}

} // namespace

int main() {
	// Keep the protocol on the original stdout; Mesen's log output (std::cout) goes to stderr.
	int protocol = dup(STDOUT_FILENO);
	if(protocol < 0 || dup2(STDERR_FILENO, STDOUT_FILENO) < 0 || !(reply = fdopen(protocol, "w"))) {
		perror("mesen-ref");
		return 1;
	}

	char line[4096];
	while(fgets(line, sizeof(line), stdin)) {
		line[strcspn(line, "\r\n")] = '\0';
		char* command = strtok(line, " ");
		if(!command) {
			continue;
		}
		if(!strcmp(command, "quit")) {
			break;
		}
		if(!strcmp(command, "name")) {
			fprintf(reply, "ok mesen2\n");
			fflush(reply);
			continue;
		}
		if(!strcmp(command, "load")) {
			char* rom = strtok(nullptr, " ");
			char* bios = strtok(nullptr, " ");
			if(!rom || !bios) {
				reply_err("usage: load ROM BIOS (Mesen needs a BIOS image)");
			} else {
				load(rom, bios);
			}
			continue;
		}
		if(!emulator) {
			reply_err("no ROM loaded");
			continue;
		}
		if(!strcmp(command, "keys")) {
			char* mask = strtok(nullptr, " ");
			set_keys(mask ? (uint16_t)strtoul(mask, nullptr, 0) : 0);
		} else if(!strcmp(command, "frame")) {
			console()->RunFrame();
			fprintf(reply, "ok %u\n", console()->GetPpu()->GetFrameCount());
			fflush(reply);
		} else if(!strcmp(command, "step")) {
			exec_one();
			reply_ok();
		} else if(!strcmp(command, "regs")) {
			regs();
		} else if(!strcmp(command, "read")) {
			char* address = strtok(nullptr, " ");
			char* length = strtok(nullptr, " ");
			if(!address || !length) {
				reply_err("usage: read ADDR LEN");
			} else {
				read_memory((uint32_t)strtoul(address, nullptr, 0), (uint32_t)strtoul(length, nullptr, 0));
			}
		} else if(!strcmp(command, "fb")) {
			frame_buffer();
		} else if(!strcmp(command, "sync")) {
			char* count = strtok(nullptr, " ");
			sync_user(count ? strtoul(count, nullptr, 0) : 1);
		} else if(!strcmp(command, "write16") || !strcmp(command, "video") || !strcmp(command, "rtc")) {
			reply_err("unsupported by mesen-ref");
		} else {
			reply_err("unknown command");
		}
	}
	unload();
	return 0;
}
