use crate::binary::BinaryReader;
use crate::parser::component_model::validator::{Validator, ValidatorStateImpl};
use crate::runtime::component_model::instantiate::InstantiateInstr;

pub struct ParseContext<'a, R, V>
where
    R: BinaryReader,
    V: ValidatorStateImpl,
{
    pub reader: &'a mut R,
    pub instrs: &'a mut Vec<InstantiateInstr>,
    pub validator: &'a mut Validator<V>,
}

impl<'a, R, V> ParseContext<'a, R, V>
where
    R: BinaryReader,
    V: ValidatorStateImpl,
{
    pub fn new(
        reader: &'a mut R,
        instrs: &'a mut Vec<InstantiateInstr>,
        validator: &'a mut Validator<V>,
    ) -> Self {
        Self {
            reader,
            instrs,
            validator,
        }
    }

    pub fn push_instr(&mut self, instr: InstantiateInstr) {
        self.instrs.push(instr);
    }

    pub fn extend_instr(&mut self, instrs: impl Iterator<Item = InstantiateInstr>) {
        self.instrs.extend(instrs);
    }
}
