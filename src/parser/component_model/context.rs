use crate::binary::BinaryReader;
use crate::component_model::CompiledState;
use crate::parser::component_model::validator::{Validator};
use crate::runtime::component_model::instantiate::InstantiateInstr;

pub struct ParseContext<'a, R>
where
    R: BinaryReader,
{
    pub reader: &'a mut R,
    pub instrs: &'a mut Vec<InstantiateInstr>,
    pub validator: &'a mut Validator<'a>,
    pub state: &'a mut CompiledState,
}

impl<'a, R> ParseContext<'a, R>
where
    R: BinaryReader,
{
    pub fn new(
        reader: &'a mut R,
        instrs: &'a mut Vec<InstantiateInstr>,
        validator: &'a mut Validator<'a>,
        state: &'a mut CompiledState,
    ) -> Self {
        Self {
            reader,
            instrs,
            validator,
            state,
        }
    }

    pub fn push_instr(&mut self, instr: InstantiateInstr) {
        self.instrs.push(instr);
    }

    pub fn extend_instr(&mut self, instrs: impl Iterator<Item = InstantiateInstr>) {
        self.instrs.extend(instrs);
    }
}
