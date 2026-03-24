use super::*;

pub(super) fn build_instruction_ordinal_by_raw_start(
    len: usize,
    instruction_starts: &[usize],
) -> Arc<[u32]> {
    let mut map = vec![u32::MAX; len];
    for (instruction_ordinal, &start) in instruction_starts.iter().enumerate() {
        map[start] = instruction_ordinal as u32;
    }
    Arc::from(map)
}

pub(super) fn collect_control_flow_metadata(
    code: &[Instr],
    instruction_starts: &[usize],
    frame_stack_base: u32,
) -> Arc<[ControlFlowMetadataSite]> {
    let mut metadata = Vec::new();
    for &start in instruction_starts {
        let op = unsafe { code[start].op };
        if let Some(rewrite) = structured_jump_rewrite(op) {
            match rewrite.kind {
                StructuredJumpRewriteKind::Single { jump_slot } => {
                    let target = unsafe { code[start + jump_slot as usize].operand.jump_addr };
                    metadata.push(ControlFlowMetadataSite {
                        instruction_ordinal: start as u32,
                        kind: ControlFlowMetadataKind::Jump {
                            jump_operand_slots: Arc::from([jump_slot]),
                            target_ordinals: Arc::from([target]),
                        },
                    });
                }
                StructuredJumpRewriteKind::BrTable => {
                    let table_size = unsafe { code[start + 1].operand.u32 as usize };
                    let mut jump_slots = Vec::with_capacity(table_size + 1);
                    let mut target_ordinals = Vec::with_capacity(table_size + 1);
                    for slot in 0..=table_size {
                        let jump_slot =
                            u8::try_from(slot + 2).expect("br_table jump slot exceeds u8");
                        jump_slots.push(jump_slot);
                        target_ordinals.push(unsafe {
                            code[start + usize::from(jump_slot)].operand.jump_addr
                        });
                    }
                    metadata.push(ControlFlowMetadataSite {
                        instruction_ordinal: start as u32,
                        kind: ControlFlowMetadataKind::Jump {
                            jump_operand_slots: Arc::from(jump_slots),
                            target_ordinals: Arc::from(target_ordinals),
                        },
                    });
                }
            }
            continue;
        }
        if let Some(shape) = loop_shape_op(op) {
            let loop_param = unsafe { code[start + 1].operand.loop_param };
            metadata.push(ControlFlowMetadataSite {
                instruction_ordinal: start as u32,
                kind: ControlFlowMetadataKind::Loop {
                    dst_from_local_top: frame_stack_base + loop_param.stack_top,
                    param_size: loop_param.param_size(),
                    shape,
                },
            });
            continue;
        }
        if let Some(shape) = block_return_shape_op(op) {
            let block_return = unsafe { code[start + 1].operand.block_return };
            metadata.push(ControlFlowMetadataSite {
                instruction_ordinal: start as u32,
                kind: ControlFlowMetadataKind::BlockReturn {
                    dst_from_local_top: frame_stack_base + block_return.stack_top,
                    return_size: block_return.return_size(),
                    shape,
                },
            });
        }
    }
    Arc::from(metadata)
}

fn map_raw_start_to_instruction_ordinal(
    raw_start: usize,
    old_to_new: &[u32],
    instruction_starts: &[usize],
) -> Option<u32> {
    let new_start = *old_to_new.get(raw_start)? as usize;
    let ordinal = instruction_starts.binary_search(&new_start).ok()?;
    Some(ordinal as u32)
}

pub(super) fn collect_stack_map_metadata(
    source_sites: &[StackMapSourceSite],
    old_to_new: &[u32],
    instruction_starts: &[usize],
) -> Arc<[StackMapSite]> {
    let mut sites = Vec::with_capacity(source_sites.len());
    for site in source_sites {
        let Some(instruction_ordinal) =
            map_raw_start_to_instruction_ordinal(site.raw_start, old_to_new, instruction_starts)
        else {
            continue;
        };
        sites.push(StackMapSite {
            instruction_ordinal,
            kind: site.kind,
            operand_bytes: site.operand_bytes,
            ref_offsets_from_operand_base: site.ref_offsets_from_operand_base.clone(),
        });
    }
    Arc::from(sites)
}

pub(super) fn collect_unwind_metadata(
    source_sites: &[UnwindSourceSite],
    old_to_new: &[u32],
    instruction_starts: &[usize],
) -> Arc<[UnwindSiteMetadata]> {
    let mut sites = Vec::with_capacity(source_sites.len());
    for site in source_sites {
        let Some(instruction_ordinal) =
            map_raw_start_to_instruction_ordinal(site.raw_start, old_to_new, instruction_starts)
        else {
            continue;
        };
        sites.push(UnwindSiteMetadata {
            instruction_ordinal,
            kind: site.kind,
            result_slot_from_local_top: site.result_slot_from_local_top,
        });
    }
    Arc::from(sites)
}
