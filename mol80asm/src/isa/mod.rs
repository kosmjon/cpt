//! Instruction-set facade: the authoritative MOL80 <-> opcode <-> standard-8080 table and the
//! encoder that turns an AST instruction into opcode bytes.

pub mod encode;
pub mod table;

pub use encode::{
    encode_alu, encode_dad, encode_dcx, encode_exchange, encode_inx, encode_move, find_mnemonic,
    EncodeError,
};
pub use table::{
    cmp_reg, cond_jump, cond_return, pair_name, pop_opcode, push_opcode, CondJumpInfo,
    CondRetInfo, IsaEntry, Pattern, CALL, ISA, JMP, RET,
};
