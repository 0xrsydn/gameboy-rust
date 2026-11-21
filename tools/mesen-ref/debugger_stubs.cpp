// Link stubs for mesen-ref. Original code for this repository; see mesen_ref.cpp.
//
// The headless Mesen build leaves out the disassembler and expression evaluator, whose
// sources still dispatch to the deleted NES/SNES cores. Mesen's Debugger class references
// them, but mesen-ref never creates a debugger, so none of these can run. Each one stops
// the process if that ever changes.

#include <cstdio>
#include <cstdlib>

#include "pch.h"
#include "Debugger/Disassembler.h"
#include "Debugger/DisassemblyInfo.h"
#include "Debugger/ExpressionEvaluator.h"

[[noreturn]] static void unavailable(const char* name) {
	fprintf(stderr, "mesen-ref: %s is not built into this reference\n", name);
	abort();
}

Disassembler::Disassembler(IConsole*, Debugger*) { unavailable("Disassembler"); }
uint32_t Disassembler::BuildCache(AddressInfo&, uint8_t, CpuType) { unavailable("Disassembler::BuildCache"); }
void Disassembler::ResetPrgCache() { unavailable("Disassembler::ResetPrgCache"); }
void Disassembler::InvalidateCache(AddressInfo, CpuType) { unavailable("Disassembler::InvalidateCache"); }
DisassemblerSource& Disassembler::GetSource(MemoryType) { unavailable("Disassembler::GetSource"); }
void Disassembler::GetLineData(DisassemblyResult&, CpuType, MemoryType, CodeLineData&) { unavailable("Disassembler::GetLineData"); }
int32_t Disassembler::GetMatchingRow(vector<DisassemblyResult>&, uint32_t, bool) { unavailable("Disassembler::GetMatchingRow"); }
vector<DisassemblyResult> Disassembler::Disassemble(CpuType, uint16_t) { unavailable("Disassembler::Disassemble"); }
uint16_t Disassembler::GetMaxBank(CpuType) { unavailable("Disassembler::GetMaxBank"); }

DisassemblyInfo::DisassemblyInfo() {}
void DisassemblyInfo::Initialize(uint32_t, uint8_t, CpuType, MemoryType, MemoryDumper*) { unavailable("DisassemblyInfo::Initialize"); }
bool DisassemblyInfo::IsInitialized() { return false; }
void DisassemblyInfo::GetDisassembly(string&, uint32_t, LabelManager*, EmuSettings*) { unavailable("DisassemblyInfo::GetDisassembly"); }
uint8_t DisassemblyInfo::GetOpSize() { unavailable("DisassemblyInfo::GetOpSize"); }
uint8_t DisassemblyInfo::GetFlags() { unavailable("DisassemblyInfo::GetFlags"); }
uint8_t* DisassemblyInfo::GetByteCode() { unavailable("DisassemblyInfo::GetByteCode"); }
void DisassemblyInfo::GetByteCode(uint8_t[8]) { unavailable("DisassemblyInfo::GetByteCode"); }
void DisassemblyInfo::GetByteCode(string&) { unavailable("DisassemblyInfo::GetByteCode"); }
EffectiveAddressInfo DisassemblyInfo::GetEffectiveAddress(Debugger*, void*, CpuType) { unavailable("DisassemblyInfo::GetEffectiveAddress"); }
uint32_t DisassemblyInfo::GetMemoryValue(EffectiveAddressInfo, MemoryDumper*, MemoryType) { unavailable("DisassemblyInfo::GetMemoryValue"); }

ExpressionEvaluator::ExpressionEvaluator(Debugger*, IDebugger*, CpuType) { unavailable("ExpressionEvaluator"); }
int64_t ExpressionEvaluator::Evaluate(ExpressionData&, EvalResultType&, MemoryOperationInfo&, AddressInfo&) { unavailable("ExpressionEvaluator::Evaluate"); }
int64_t ExpressionEvaluator::Evaluate(string, EvalResultType&, MemoryOperationInfo&, AddressInfo&) { unavailable("ExpressionEvaluator::Evaluate"); }
ExpressionData ExpressionEvaluator::GetRpnList(string, bool&) { unavailable("ExpressionEvaluator::GetRpnList"); }
void ExpressionEvaluator::GetTokenList(char*) { unavailable("ExpressionEvaluator::GetTokenList"); }
