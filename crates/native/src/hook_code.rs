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
    PrefixChanged,
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
