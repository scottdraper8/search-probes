use iced_x86::{
    BlockEncoder, BlockEncoderOptions, Code, Decoder, DecoderOptions, FlowControl, Instruction,
    InstructionBlock,
};

pub const JUMP_SIZE: usize = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeError {
    PrefixLength,
    InvalidInstruction,
    ControlFlow,
    Relocation,
}

pub fn absolute_jump(destination: u64) -> [u8; JUMP_SIZE] {
    let mut code = [0; JUMP_SIZE];
    code[..2].copy_from_slice(&[0xff, 0x25]);
    code[6..].copy_from_slice(&destination.to_le_bytes());
    code
}

pub fn relocate_prefix(source: u64, prefix: &[u8], destination: u64) -> Result<Vec<u8>, CodeError> {
    if !(JUMP_SIZE..=64).contains(&prefix.len()) {
        return Err(CodeError::PrefixLength);
    }
    let mut instructions = Vec::new();
    let mut decoder = Decoder::with_ip(64, prefix, source, DecoderOptions::NONE);
    while decoder.can_decode() {
        let instruction = decoder.decode();
        if instruction.is_invalid() {
            return Err(CodeError::InvalidInstruction);
        }
        if instruction.flow_control() != FlowControl::Next {
            return Err(CodeError::ControlFlow);
        }
        instructions.push(instruction);
    }
    let continuation = Instruction::with_branch(Code::Jmp_rel32_64, source + prefix.len() as u64)
        .map_err(|_| CodeError::Relocation)?;
    instructions.push(continuation);
    BlockEncoder::encode(
        64,
        InstructionBlock::new(&instructions, destination),
        BlockEncoderOptions::NONE,
    )
    .map(|encoded| encoded.code_buffer)
    .map_err(|_| CodeError::Relocation)
}

#[cfg(test)]
mod tests {
    use super::*;
    const PREFIX: [u8; 17] = [
        0x48, 0x89, 0x5c, 0x24, 0x08, 0x57, 0x48, 0x83, 0xec, 0x20, 0x48, 0x8b, 0x91, 0x20, 0x34,
        0, 0,
    ];

    #[test]
    fn absolute_jump_preserves_registers_and_full_address() {
        let address = 0x1234_5678_9abc_def0;
        let code = absolute_jump(address);
        assert_eq!(&code[..6], &[0xff, 0x25, 0, 0, 0, 0]);
        assert_eq!(u64::from_le_bytes(code[6..].try_into().unwrap()), address);
    }

    #[test]
    fn straight_prologue_relocates_beyond_relative_jump_range() {
        let source = 0x1401cea50;
        let destination = 0x7000_0000_0000;
        let code = relocate_prefix(source, &PREFIX, destination).unwrap();
        assert_eq!(&code[..PREFIX.len()], &PREFIX);
        let mut decoder = Decoder::with_ip(
            64,
            &code[PREFIX.len()..],
            destination + PREFIX.len() as u64,
            DecoderOptions::NONE,
        );
        let jump = decoder.decode();
        assert_eq!(jump.flow_control(), FlowControl::IndirectBranch);
        let literal = (jump.ip_rel_memory_address() - destination) as usize;
        assert_eq!(
            u64::from_le_bytes(code[literal..literal + 8].try_into().unwrap()),
            source + PREFIX.len() as u64
        );
    }

    #[test]
    fn calls_in_the_overwritten_prefix_are_rejected() {
        let mut prefix = [0x90; 14];
        prefix[..5].copy_from_slice(&[0xe8, 0, 0, 0, 0]);
        assert_eq!(
            relocate_prefix(0x140000000, &prefix, 0x150000000),
            Err(CodeError::ControlFlow)
        );
    }

    #[test]
    fn partial_instruction_is_rejected() {
        assert_eq!(
            relocate_prefix(0x140000000, &PREFIX[..16], 0x150000000),
            Err(CodeError::InvalidInstruction)
        );
    }

    #[test]
    fn unreachable_rip_operand_fails_explicitly() {
        let mut prefix = vec![0x48, 0x8b, 0x05, 0x10, 0, 0, 0];
        prefix.extend_from_slice(&[0x90; 7]);
        assert_eq!(
            relocate_prefix(0x140000000, &prefix, 0x700000000000),
            Err(CodeError::Relocation)
        );
    }
}
