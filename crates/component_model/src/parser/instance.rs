use crate::name::{ExportName, ImportName};
use crate::parser::ComponentParser;
use crate::parser::component::RawData;
use crate::parser::idx::RawComponentIdx;
use crate::parser::sort::{CoreSort, Sort, SortType};
use crate::{ComponentParseError, Result};
use binary_reader::BinaryReader;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use tracing::trace;

use crate::types::component::ComponentType;
use crate::types::instance::{ChildImportLookup, InstanceArg, InstanceType, ParentExportLookup};
use crate::types::resource::{ResourceDef, ResourcePlan};
use crate::types::{ResourceDefId, ResourceTableId, TypeId, TypeResourceTableIndex, TypeValidator};

pub struct RawInstance {
    pub component_idx: RawComponentIdx,
    pub args: Vec<(ImportName, Sort)>,
}

pub struct RawInstanceInlineExport {
    pub exports: Vec<(ExportName, Sort)>,
}

pub enum RawInstanceDef {
    Instantiate(RawInstance),
    InlineExport(RawInstanceInlineExport),
}

impl<T> ComponentParser<'_, T>
where
    T: BinaryReader,
{
    pub(crate) fn parse_instance(&mut self) -> Result<()> {
        trace!("parse_instance");
        match self.reader.read_exact_one()? {
            0x00 => self.parse_instantiate(),
            0x01 => self.parse_instantiate_inline_export(),
            x => Err(ComponentParseError::InvalidInstanceType(x)),
        }
    }

    fn parse_instantiate(&mut self) -> Result<()> {
        trace!("parse_instantiate");
        let component_idx = self.parse_component_idx()?;
        // NOTE: 今は後からsortからtype_idを取得するようにしているが、最初からやってArgをEnumとして渡すようにしたほうがいいかも
        let args = {
            let mut name_unique = HashSet::new();
            self.parse_vec(move |slf| {
                let (name, sort) = slf.parse_instantiate_arg()?;
                if name_unique.contains(&name) {
                    Err(ComponentParseError::InvalidName(
                        "Duplicated target import name".to_owned(),
                    ))?
                } else {
                    name_unique.insert(name.clone());
                    Ok((name, sort))
                }
            })?
        };
        let component_type_id = self.validator.locals.get_component_type(&component_idx)?;
        let mut ty = InstanceType {
            component_type_id,
            resolved_tables: Default::default(),
            table_map: vec![],
        };
        let mut type_args = Vec::new();
        for (name, sort) in &args {
            let type_id = self.get_type_id_from_sort(sort)?.expect("todo: core type");
            let arg = InstanceArg {
                ty: type_id,
                sort: sort.get_type(),
                name: name.clone(),
            };
            type_args.push(arg);
        }
        let component_type = self.validator.store.get_component(&component_type_id)?;
        bind_instance_tables(component_type, &mut ty, type_args)?;
        let instance = RawInstance {
            component_idx,
            args,
        };
        let idx = self
            .instances
            .push(RawData::Defined(RawInstanceDef::Instantiate(instance)))?;
        let id = self.validator.store.push_instance_in_type(ty);
        self.validator.locals.push_instance(idx, id);
        Ok(())
    }

    fn parse_instantiate_arg(&mut self) -> Result<(ImportName, Sort)> {
        let name = self.parse_import_name()?;
        let sort = self.parse_sort()?;
        Ok((name, sort))
    }

    fn parse_instantiate_inline_export(&mut self) -> Result<()> {
        let mut name_unique = HashSet::new();
        let exports = self.parse_vec(|slf| {
            let (name, sort) = slf.parse_inline_export_arg()?;
            if name_unique.contains(&name) {
                Err(ComponentParseError::InvalidName(
                    "Duplicated inline export name".to_owned(),
                ))?
            } else {
                name_unique.insert(name.clone());
                Ok((name, sort))
            }
        })?;
        let instance = RawInstanceInlineExport { exports };
        self.instances
            .push(RawData::Defined(RawInstanceDef::InlineExport(instance)))?;
        Ok(())
    }

    fn parse_inline_export_arg(&mut self) -> Result<(ExportName, Sort)> {
        let name = self.parse_export_name()?;
        let sort = self.parse_sort()?;
        Ok((name, sort))
    }
}

fn invert_plan(plan: &ResourcePlan) -> IndexMap<TypeResourceTableIndex, ResourceDefId> {
    let mut map = IndexMap::new();
    for (k, idx) in plan.table_index_of_key.iter() {
        map.insert(*idx, k.clone());
    }
    map.sort_unstable_keys();
    map
}

fn concretize_key(
    key: &ResourceDefId,
    subst: &IndexMap<ResourceDefId, ResourceDefId>,
) -> ResourceDefId {
    *subst.get(key).unwrap_or(key)
}

fn bind_instance_tables(
    child: &ComponentType,   // instantiate される子
    inst: &mut InstanceType, // 子の Instance ノード（args 済み）
    args: Vec<InstanceArg>,
) -> Result<()> {
    let mut subst: IndexMap<ResourceDefId, ResourceDefId> = IndexMap::new();
    // todo: sub resource以外の場合は型を比較してchildがparentのsubtypeかを確認する
    // eq typeの場合もresourceと同じことをしたい．後々共通化とかできたら嬉しい
    for arg in args {
        if arg.sort != SortType::Type {
            continue;
        }

        if let TypeId::Resource(parent_def_id) = arg.ty {
            let child_def_id = child
                .surface
                .imports
                .get(&arg.name)
                .unwrap()
                .ensure_sub_resource()?;
            subst.insert(parent_def_id, child_def_id);
        }
    }

    let idx2key = invert_plan(&child.plan);
    let mut unique: IndexMap<ResourceDefId, ResourceTableId> = IndexMap::new();
    let mut table_map: Vec<ResourceTableId> = vec![ResourceTableId(u32::MAX); idx2key.len()];
    let mut next_local: u32 = 0;

    for (tidx, key) in idx2key {
        let ck = concretize_key(&key, &subst);
        let entry = unique.entry(ck.clone()).or_insert_with(|| {
            let id = ResourceTableId(next_local);
            next_local += 1;
            id
        });
        let pos = tidx.0 as usize;
        if pos >= table_map.len() {
            table_map.resize(pos + 1, ResourceTableId(u32::MAX));
        }
        table_map[pos] = *entry;
    }

    inst.resolved_tables = unique;
    inst.table_map = table_map;
    Ok(())
}
