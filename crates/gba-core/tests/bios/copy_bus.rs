//! Copy services execute real bus accesses, including ignored BIOS destinations.
use super::*;

#[test]
fn copy_and_fill_to_bios_return_without_modifying_firmware() {
    for thumb in [false, true] {
        for (service, width) in [(0x0b, 0), (0x0b, 1 << 26), (0x0c, 0)] {
            for fill in [0, 1 << 24] {
                let (mut machine, return_pc) = call(service, thumb, [SOURCE, 0, 28 | width | fill]);
                for index in 0..32 {
                    machine
                        .memory_mut()
                        .write32(SOURCE + index * 4, 0x1234_abcd ^ index)
                        .unwrap();
                }
                let before = machine.cpu().clone();
                let cycles = machine.cycles();
                finish(&mut machine, return_pc);
                assert_eq!(&machine.cpu().registers()[..15], &before.registers()[..15]);
                assert_eq!(machine.cpu().cpsr(), before.cpsr());
                assert!(machine.cycles() > cycles);
                for (address, expected) in bios::image().into_iter().enumerate() {
                    assert_eq!(machine.memory().read8(address as u32).unwrap(), expected);
                }
                assert_eq!(
                    machine.memory_mut().write16(0, 0),
                    Err(MemoryError::ReadOnly(0))
                );
            }
        }
    }
}

#[test]
fn ignored_bios_destination_does_not_bypass_invalid_source_reads() {
    for service in [0x0b, 0x0c] {
        let (mut machine, _) = call(service, true, [0x0e00_0000, 0, 1]);
        let error = (0..200)
            .find_map(|_| machine.step().err())
            .expect("source read must still fail");
        assert_eq!(
            error,
            MachineError::Cpu(CpuError::Memory(MemoryError::Unmapped(0x0e00_0000)))
        );
        let cpu = machine.cpu().clone();
        let cycles = machine.cycles();
        assert_eq!(machine.step(), Err(error));
        assert_eq!(machine.cpu(), &cpu);
        assert_eq!(machine.cycles(), cycles);
    }
}
