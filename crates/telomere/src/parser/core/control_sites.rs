use std::sync::Arc;

use crate::{
    binary::BinaryReader,
    common::{StackMapSafepointKind, StackMapSourceSite, UnwindSourceSite},
    parser::core::{instruction::InstructionParser, instruction_generator::InstructionGenerator},
};

use super::type_checker::TypeChecker;

impl<'a, R: BinaryReader> InstructionParser<'a, R> {
    pub(crate) fn record_stack_map_site(
        &self,
        instrs: &InstructionGenerator,
        checker: &TypeChecker,
        stack_map_sites: &mut Vec<StackMapSourceSite>,
        kind: StackMapSafepointKind,
    ) {
        let Some(operand_bytes) = checker.current_stack_byte_size() else {
            return;
        };
        let Some(ref_offsets_from_operand_base) = checker.current_ref_offsets_from_operand_base()
        else {
            return;
        };
        stack_map_sites.push(StackMapSourceSite {
            raw_start: instrs.len(),
            kind,
            operand_bytes,
            ref_offsets_from_operand_base: Arc::from(ref_offsets_from_operand_base),
        });
    }

    pub(crate) fn record_unwind_site(
        &self,
        instrs: &InstructionGenerator,
        unwind_sites: &mut Vec<UnwindSourceSite>,
        kind: StackMapSafepointKind,
        result_slot_from_local_top: Option<u32>,
    ) {
        unwind_sites.push(UnwindSourceSite {
            raw_start: instrs.len(),
            kind,
            result_slot_from_local_top,
        });
    }
}
