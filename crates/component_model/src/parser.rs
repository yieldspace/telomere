pub(crate) mod alias;
pub(crate) mod canon;
pub(crate) mod component;
pub(crate) mod core;
pub(crate) mod export;
pub(crate) mod idx;
pub(crate) mod import;
pub(crate) mod instance;
pub(crate) mod name;
pub(crate) mod section_type;
pub(crate) mod sort;
pub(crate) mod types;
pub(crate) mod vec;

use crate::inline::Inliner;
use crate::parser::canon::{RawCoreFunction, RawFunction};
use crate::parser::component::{RawComponent, RawCoreData, RawData};
use crate::parser::core::CoreInstanceDef;
use crate::parser::export::RawExport;
use crate::parser::idx::{
    RawComponentIdx, RawCoreFuncIdx, RawCoreGlobalIdx, RawCoreInstanceIdx, RawCoreMemoryIdx,
    RawCoreModuleIdx, RawCoreTableIdx, RawCoreTypeIdx, RawExportId, RawFuncIdx, RawImportId,
    RawInstanceIdx,
};
use crate::parser::import::RawImport;
use crate::parser::instance::{RawInstance, RawInstanceDef};
use crate::parser::section_type::ComponentSection;
use crate::parser::vec::RawIndexVec;
use crate::types::component::ComponentType;
use crate::types::resource::ResourcePlan;
use crate::types::{
    ComponentDefId, ResourceUseCollector, TypeResourceTableIndex, TypeStore, TypeValidator,
};
use crate::{Component, ComponentParseError, InstantiateContext, Result};
use binary_reader::BinaryReader;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use telomere_wasm::WasmParser;
use telomere_wasm::parser::core::parse_u32;

pub struct ComponentParser<'a, T: BinaryReader> {
    reader: &'a mut T,
    outer: Vec<ComponentDefId>,
    validator: TypeValidator<'a>,
    imports: HashMap<RawImportId, RawImport>,
    exports: HashMap<RawExportId, RawExport>,
    components: RawIndexVec<RawComponentIdx, RawData<RawComponent>>,
    instances: RawIndexVec<RawInstanceIdx, RawData<RawInstanceDef>>,
    funcs: RawIndexVec<RawFuncIdx, RawData<RawFunction>>,
    core_modules: RawIndexVec<RawCoreModuleIdx, RawCoreData<telomere_wasm::Module>>,
    core_instances: RawIndexVec<RawCoreInstanceIdx, RawCoreData<CoreInstanceDef>>,
    core_memories: RawIndexVec<RawCoreMemoryIdx, RawCoreData<()>>,
    core_globals: RawIndexVec<RawCoreGlobalIdx, RawCoreData<()>>,
    core_tables: RawIndexVec<RawCoreTableIdx, RawCoreData<()>>,
    core_types: RawIndexVec<RawCoreTypeIdx, RawCoreData<()>>,
    core_funcs: RawIndexVec<RawCoreFuncIdx, RawCoreData<RawCoreFunction>>,
}

impl<'a, T> ComponentParser<'a, T>
where
    T: BinaryReader,
{
    pub fn new(reader: &'a mut T, store: &'a mut TypeStore) -> Self {
        Self {
            reader,
            outer: vec![],
            validator: TypeValidator::new(store),
            imports: HashMap::new(),
            exports: HashMap::new(),
            components: RawIndexVec::with_capacity(256),
            instances: RawIndexVec::with_capacity(256),
            funcs: RawIndexVec::with_capacity(256),
            core_modules: RawIndexVec::with_capacity(256),
            core_instances: RawIndexVec::with_capacity(256),
            core_funcs: RawIndexVec::with_capacity(256),
            core_memories: RawIndexVec::with_capacity(256),
            core_globals: RawIndexVec::with_capacity(256),
            core_tables: RawIndexVec::with_capacity(256),
            core_types: RawIndexVec::with_capacity(256),
        }
    }

    pub fn new_with_outer(
        reader: &'a mut T,
        store: &'a mut TypeStore,
        outer: Vec<ComponentDefId>,
    ) -> Self {
        let mut parser = Self::new(reader, store);
        parser.outer = outer;
        parser
    }

    pub fn parse_vec<V>(&mut self, mut func: impl FnMut(&mut Self) -> Result<V>) -> Result<Vec<V>> {
        let (_, count) = parse_u32(self.reader)?;
        let mut vec = Vec::with_capacity(count as usize);
        for _ in 0..count {
            vec.push(func(self)?);
        }
        Ok(vec)
    }

    pub fn parse_option(&mut self) -> Result<Option<&mut Self>> {
        match self.reader.read_exact_one()? {
            0x00 => Ok(None),
            0x01 => Ok(Some(self)),
            x => Err(ComponentParseError::InvalidSignature(
                Box::new([x]),
                Box::new([0x00]),
                "option".to_string(),
            )),
        }
    }

    fn parse_magic(&mut self) -> Result<()> {
        let magic = self.reader.read_exact::<4>()?;
        if matches!(&magic, &[0x00, 0x61, 0x73, 0x6d]) {
            Ok(())
        } else {
            Err(ComponentParseError::InvalidSignature(
                Box::new(magic),
                Box::new([0x00, 0x61, 0x73, 0x6d]),
                "magic".to_string(),
            ))
        }
    }

    fn parse_version(&mut self) -> Result<()> {
        let version = self.reader.read_exact::<2>()?;
        if matches!(&version, &[0x0d, 0x00]) {
            Ok(())
        } else {
            Err(ComponentParseError::InvalidSignature(
                Box::new(version),
                Box::new([0x0d, 0x00]),
                "version".to_string(),
            ))
        }
    }

    fn parse_layer(&mut self) -> Result<()> {
        let layer = self.reader.read_exact::<2>()?;
        if matches!(&layer, &[0x01, 0x00]) {
            Ok(())
        } else {
            Err(ComponentParseError::InvalidSignature(
                Box::new(layer),
                Box::new([0x01, 0x00]),
                "layer".to_string(),
            ))
        }
    }

    fn parse_section_type(&mut self) -> Result<Option<ComponentSection>> {
        if let Some(kind) = self.reader.read_one()? {
            match kind {
                0x00 => Ok(Some(ComponentSection::Custom)),
                0x01 => Ok(Some(ComponentSection::CoreModule)),
                0x02 => Ok(Some(ComponentSection::CoreInstance)),
                0x03 => Ok(Some(ComponentSection::CoreType)),
                0x04 => Ok(Some(ComponentSection::Component)),
                0x05 => Ok(Some(ComponentSection::Instance)),
                0x06 => Ok(Some(ComponentSection::Alias)),
                0x07 => Ok(Some(ComponentSection::Type)),
                0x08 => Ok(Some(ComponentSection::Canon)),
                0x09 => Ok(Some(ComponentSection::Start)),
                0x0a => Ok(Some(ComponentSection::Import)),
                0x0b => Ok(Some(ComponentSection::Export)),
                #[cfg(feature = "value-imports-exports")]
                0x0c => Ok(Some(ComponentSection::Value)),
                _ => Err(ComponentParseError::InvalidSignature(
                    Box::new([kind]),
                    Box::new([0x00]),
                    "section type".into(),
                )),
            }
        } else {
            Ok(None)
        }
    }

    pub fn parse(self) -> Result<RawComponent> {
        let (raw_component, ty) = self.parse_component()?;
        println!("{:#?}", ty);
        Ok(raw_component)
    }

    fn parse_component(mut self) -> Result<(RawComponent, ComponentType)> {
        self.parse_magic()?;
        self.parse_version()?;
        self.parse_layer()?;

        while let Some(st) = self.parse_section_type()? {
            let (_, section_size) = parse_u32(self.reader)?;
            match st {
                ComponentSection::Custom => {
                    let mut data = Vec::with_capacity(section_size as usize);
                    assert_eq!(
                        section_size as usize,
                        self.reader.read_slice(data.as_mut_slice())?
                    );
                }
                ComponentSection::CoreModule => {
                    let module = {
                        let mut sized_reader = self.reader.take(section_size as usize);
                        let mut core_module = WasmParser::new(&mut sized_reader);
                        core_module.parse_module()
                    }?;
                    let _idx = self.core_modules.push(RawCoreData::Defined(module));
                }
                ComponentSection::CoreInstance => {
                    let (_, count) = parse_u32(self.reader)?;
                    for _ in 0..count {
                        self.parse_core_instance()?;
                    }
                }
                ComponentSection::CoreType => todo!(),
                ComponentSection::Component => {
                    let (component, component_ty) = {
                        let mut sized_reader = self.reader.take(section_size as usize);
                        let mut outer = self.outer.clone();
                        outer.push(self.validator.id);
                        let parser = ComponentParser::new_with_outer(
                            &mut sized_reader,
                            self.validator.store,
                            outer,
                        );
                        parser.parse_component()?
                    };
                    let idx = self.components.push(RawData::Defined(component))?;
                    let id = self.validator.store.push_component_in_type(component_ty);
                    self.validator.locals.push_component(idx, id);
                }
                ComponentSection::Instance => {
                    let (_, count) = parse_u32(self.reader)?;
                    for _ in 0..count {
                        self.parse_instance()?
                    }
                }
                ComponentSection::Alias => {
                    let (_, count) = parse_u32(self.reader)?;
                    for _ in 0..count {
                        self.parse_alias()?
                    }
                }
                ComponentSection::Type => {
                    let (_, count) = parse_u32(self.reader)?;
                    for _ in 0..count {
                        self.parse_type()?;
                    }
                }
                ComponentSection::Canon => {
                    let (_, count) = parse_u32(self.reader)?;
                    for _ in 0..count {
                        self.parse_canon()?;
                    }
                }
                ComponentSection::Start => todo!(),
                ComponentSection::Import => {
                    let (_, count) = parse_u32(self.reader)?;
                    for _ in 0..count {
                        self.parse_import()?;
                    }
                }
                ComponentSection::Export => {
                    let (_, count) = parse_u32(self.reader)?;
                    for _ in 0..count {
                        self.parse_export()?;
                    }
                }
            }
        }

        {
            let Self {
                validator,
                imports,
                exports,
                components,
                instances,
                funcs,
                core_modules,
                core_instances,
                core_memories,
                core_globals,
                core_tables,
                core_types,
                core_funcs,
                ..
            } = self;
            let TypeValidator {
                id,
                usec,
                store: _,
                locals,
                surface,
            } = validator;

            let mut plan = ResourcePlan::default();
            finalize_plan(&mut plan, usec);
            let component = RawComponent {
                imports,
                exports,
                ops: vec![],
                components,
                instances,
                funcs,
                core_modules,
                core_instances,
                core_memories,
                core_globals,
                core_tables,
                core_types,
                core_funcs,
            };
            let ty = ComponentType {
                id,
                local_type_map: locals,
                plan,
                surface,
            };
            Ok((component, ty))
        }
    }
}

fn finalize_plan(plan: &mut ResourcePlan, usec: ResourceUseCollector) {
    for (i, k) in usec.used.into_iter().enumerate() {
        plan.table_index_of_key
            .insert(k, TypeResourceTableIndex(i as u32));
    }
}
