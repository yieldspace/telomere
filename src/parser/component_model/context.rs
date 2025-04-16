use crate::binary::BinaryReader;
use crate::component_model::CoreInstance;
use crate::parser::component_model::validator::Validator;
use crate::runtime::component_model::instantiate::InstantiateInstr;
use crate::Module;

pub struct ParseContext<'a, R, V>
where
    R: BinaryReader,
    V: Validator,
{
    pub reader: &'a mut R,
    pub instrs: &'a mut Vec<InstantiateInstr>,
    pub validator: &'a mut V,
}

impl<'a, R, V> ParseContext<'a, R, V>
where
    R: BinaryReader,
    V: Validator,
{
    pub fn new(
        reader: &'a mut R,
        instrs: &'a mut Vec<InstantiateInstr>,
        validator: &'a mut V,
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

    pub fn add_core_module(&mut self, module: Module) {
        todo!()
    }

    pub fn add_core_instance(&mut self, instance: CoreInstance) {}
}
