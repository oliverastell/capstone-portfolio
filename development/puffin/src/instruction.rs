use enum_assoc::Assoc;
use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::FromPrimitive;

#[repr(u8)]
#[derive(Copy, Clone, Debug, FromPrimitive, ToPrimitive)]
pub(crate) enum Instruction {
    Exit,
    Return,

    Load64,
    Load32,
    Load16,
    Load8,
    Load,
    
    Call,
}