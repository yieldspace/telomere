use crate::binary::BinaryReader;
use crate::parser::component_model::validator::Validator;
use crate::runtime::component_model::instantiate::InstantiateInstr;

pub struct ParseContext<'a, R>
where
    R: BinaryReader,
{
    pub reader: &'a mut R,
    pub instrs: &'a mut Vec<InstantiateInstr>,
    pub validator: &'a mut dyn Validator,
}

impl<'a, R> ParseContext<'a, R>
where
    R: BinaryReader,
{
    pub fn new(
        reader: &'a mut R,
        instrs: &'a mut Vec<InstantiateInstr>,
        validator: &'a mut dyn Validator,
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
