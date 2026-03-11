use heck::{ToSnakeCase, ToUpperCamelCase};
use proc_macro2::{Ident, Span, TokenStream as TokenStream2};
use proc_macro_crate::{crate_name, FoundCrate};
use quote::quote;
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;
use syn::parse::{Parse, ParseStream};
use syn::{braced, parse_quote, Error, LitStr, Result, Token};
use wit_parser::{
    Function, FunctionKind, InterfaceId, Resolve, Type, TypeDefKind, TypeId, TypeOwner, WorldId,
    WorldItem,
};

pub struct BindgenInput {
    inline: Option<LitStr>,
    path: Option<LitStr>,
    world: LitStr,
    module: Option<LitStr>,
}

impl Parse for BindgenInput {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let content;
        braced!(content in input);

        let mut inline = None;
        let mut path = None;
        let mut world = None;
        let mut module = None;

        while !content.is_empty() {
            let key = content.parse::<Ident>()?;
            content.parse::<Token![:]>()?;
            let value = content.parse::<LitStr>()?;
            match key.to_string().as_str() {
                "inline" => set_once(&mut inline, value, "inline")?,
                "path" => set_once(&mut path, value, "path")?,
                "world" => set_once(&mut world, value, "world")?,
                "module" => set_once(&mut module, value, "module")?,
                other => {
                    return Err(Error::new(
                        key.span(),
                        format!("unsupported bindgen option `{other}`"),
                    ));
                }
            }

            if content.is_empty() {
                break;
            }
            content.parse::<Token![,]>()?;
        }

        let world = world.ok_or_else(|| Error::new(Span::call_site(), "`world` is required"))?;
        if inline.is_some() == path.is_some() {
            return Err(Error::new(
                Span::call_site(),
                "specify exactly one of `inline` or `path`",
            ));
        }

        Ok(Self {
            inline,
            path,
            world,
            module,
        })
    }
}

fn set_once(slot: &mut Option<LitStr>, value: LitStr, name: &str) -> Result<()> {
    if slot.replace(value).is_some() {
        return Err(Error::new(
            Span::call_site(),
            format!("`{name}` specified more than once"),
        ));
    }
    Ok(())
}

pub fn expand(input: BindgenInput) -> Result<TokenStream2> {
    let runtime_path = resolve_crate_path("telomere-component");
    let module_ident = input
        .module
        .as_ref()
        .map(|value| snake_ident(&value.value()))
        .unwrap_or_else(|| snake_ident(last_segment(&input.world.value())));

    let (mut resolve, world_id) = load_world(&input)?;
    resolve.generate_nominal_type_ids(world_id);

    let generator = Generator::new(resolve, world_id, runtime_path)?;
    let body = generator.generate()?;
    Ok(quote! {
        pub mod #module_ident {
            #body
        }
    })
}

fn load_world(input: &BindgenInput) -> Result<(Resolve, WorldId)> {
    let mut resolve = Resolve::default();
    let package = if let Some(inline) = &input.inline {
        resolve
            .push_source("<inline>.wit", &inline.value())
            .map_err(|error| Error::new(inline.span(), error.to_string()))?
    } else {
        let path = input.path.as_ref().unwrap();
        let manifest_dir = env::var("CARGO_MANIFEST_DIR")
            .map_err(|error| Error::new(path.span(), error.to_string()))?;
        let absolute = PathBuf::from(manifest_dir).join(path.value());
        let (package, _) = resolve
            .push_path(&absolute)
            .map_err(|error| Error::new(path.span(), error.to_string()))?;
        package
    };

    let world_id = resolve
        .select_world(&[package], Some(&input.world.value()))
        .map_err(|error| Error::new(input.world.span(), error.to_string()))?;
    Ok((resolve, world_id))
}

fn resolve_crate_path(name: &str) -> syn::Path {
    match crate_name(name) {
        Ok(FoundCrate::Itself) => parse_quote!(crate),
        Ok(FoundCrate::Name(found)) => {
            let ident = Ident::new(&found.replace('-', "_"), Span::call_site());
            parse_quote!(::#ident)
        }
        Err(_) => {
            let ident = Ident::new(&name.replace('-', "_"), Span::call_site());
            parse_quote!(::#ident)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum NamespaceLocation {
    World,
    Import(String),
    Export(String),
}

#[derive(Clone, Copy)]
enum ScopeDepth {
    Root,
    Nested,
}

#[derive(Clone)]
struct FunctionBinding {
    wit_name: String,
    method_ident: Ident,
    function: Function,
}

#[derive(Clone)]
struct InterfaceNamespace {
    wit_name: String,
    module_ident: Ident,
    location: NamespaceLocation,
    functions: Vec<FunctionBinding>,
}

struct Generator {
    resolve: Resolve,
    world_id: WorldId,
    runtime_path: syn::Path,
    direct_imports: Vec<FunctionBinding>,
    direct_exports: Vec<FunctionBinding>,
    import_interfaces: Vec<InterfaceNamespace>,
    export_interfaces: Vec<InterfaceNamespace>,
    used_type_ids: HashSet<TypeId>,
    named_type_ids: Vec<TypeId>,
    type_unique_idents: HashMap<TypeId, Ident>,
    type_alias_idents: HashMap<TypeId, Ident>,
    type_locations: HashMap<NamespaceLocation, Vec<TypeId>>,
}

impl Generator {
    fn new(resolve: Resolve, world_id: WorldId, runtime_path: syn::Path) -> Result<Self> {
        let import_items = {
            let world = &resolve.worlds[world_id];
            world
                .imports
                .iter()
                .map(|(key, item)| (resolve.name_world_key(key), item.clone()))
                .collect::<Vec<_>>()
        };
        let export_items = {
            let world = &resolve.worlds[world_id];
            world
                .exports
                .iter()
                .map(|(key, item)| (resolve.name_world_key(key), item.clone()))
                .collect::<Vec<_>>()
        };

        let mut generator = Self {
            resolve,
            world_id,
            runtime_path,
            direct_imports: Vec::new(),
            direct_exports: Vec::new(),
            import_interfaces: Vec::new(),
            export_interfaces: Vec::new(),
            used_type_ids: HashSet::new(),
            named_type_ids: Vec::new(),
            type_unique_idents: HashMap::new(),
            type_alias_idents: HashMap::new(),
            type_locations: HashMap::new(),
        };

        for (name, item) in import_items {
            generator.collect_world_item(name, item, true)?;
        }
        for (name, item) in export_items {
            generator.collect_world_item(name, item, false)?;
        }

        let mut named = generator
            .used_type_ids
            .iter()
            .copied()
            .filter(|id| generator.resolve.types[*id].name.is_some())
            .collect::<Vec<_>>();
        named.sort_by_key(|id| id.index());
        generator.named_type_ids = named;
        generator.assign_type_names();

        Ok(generator)
    }

    fn collect_world_item(
        &mut self,
        wit_name: String,
        item: WorldItem,
        is_import: bool,
    ) -> Result<()> {
        match item {
            WorldItem::Function(function) => {
                self.validate_function(&function)?;
                self.collect_function_types(&function)?;
                let binding = FunctionBinding {
                    wit_name: wit_name.clone(),
                    method_ident: snake_ident(&function.name),
                    function,
                };
                if is_import {
                    self.direct_imports.push(binding);
                } else {
                    self.direct_exports.push(binding);
                }
            }
            WorldItem::Interface { id, .. } => {
                let namespace = self.build_interface_namespace(&wit_name, id, is_import)?;
                if is_import {
                    self.import_interfaces.push(namespace);
                } else {
                    self.export_interfaces.push(namespace);
                }
            }
            WorldItem::Type { id, .. } => {
                self.collect_type_use(Type::Id(id))?;
                self.type_locations
                    .entry(NamespaceLocation::World)
                    .or_default()
                    .push(id);
            }
        }
        Ok(())
    }

    fn build_interface_namespace(
        &mut self,
        wit_name: &str,
        id: InterfaceId,
        is_import: bool,
    ) -> Result<InterfaceNamespace> {
        let interface = self.resolve.interfaces[id].clone();
        let location = if is_import {
            NamespaceLocation::Import(wit_name.to_owned())
        } else {
            NamespaceLocation::Export(wit_name.to_owned())
        };
        let mut type_ids = interface.types.values().copied().collect::<Vec<_>>();
        type_ids.sort_by_key(|type_id| type_id.index());
        for type_id in &type_ids {
            self.collect_type_use(Type::Id(*type_id))?;
        }
        for type_id in &type_ids {
            self.type_locations
                .entry(location.clone())
                .or_default()
                .push(*type_id);
        }

        let mut functions = Vec::new();
        for (name, function) in interface.functions {
            self.validate_function(&function)?;
            self.collect_function_types(&function)?;
            functions.push(FunctionBinding {
                wit_name: name.clone(),
                method_ident: snake_ident(&name),
                function,
            });
        }

        Ok(InterfaceNamespace {
            wit_name: wit_name.to_owned(),
            module_ident: snake_ident(wit_name),
            location,
            functions,
        })
    }

    fn assign_type_names(&mut self) {
        let mut used = HashSet::new();
        for type_id in &self.named_type_ids {
            let typedef = &self.resolve.types[*type_id];
            let local = camel_ident(typedef.name.as_deref().unwrap());
            self.type_alias_idents.insert(*type_id, local.clone());

            let prefix = match typedef.owner {
                TypeOwner::World(id) if id == self.world_id => "world".to_owned(),
                TypeOwner::Interface(id) => self
                    .resolve
                    .interfaces
                    .get(id)
                    .and_then(|interface| interface.name.clone())
                    .unwrap_or_else(|| format!("interface{}", id.index())),
                _ => "type".to_owned(),
            };
            let mut candidate =
                format!("{prefix} {}", typedef.name.as_deref().unwrap()).to_upper_camel_case();
            if candidate.is_empty() {
                candidate = format!("GeneratedType{}", type_id.index());
            }
            let mut unique = candidate.clone();
            let mut suffix = 1u32;
            while !used.insert(unique.clone()) {
                suffix += 1;
                unique = format!("{candidate}{suffix}");
            }
            self.type_unique_idents
                .insert(*type_id, Ident::new(&unique, Span::call_site()));
        }
    }

    fn validate_function(&self, function: &Function) -> Result<()> {
        if function.kind.is_async() {
            return Err(Error::new(
                Span::call_site(),
                format!(
                    "async WIT function `{}` is not supported by telomere-component-bindgen yet",
                    function.name
                ),
            ));
        }
        if !matches!(function.kind, FunctionKind::Freestanding) {
            return Err(Error::new(
                Span::call_site(),
                format!(
                    "resource function `{}` is not supported by telomere-component-bindgen yet",
                    function.name
                ),
            ));
        }
        Ok(())
    }

    fn collect_function_types(&mut self, function: &Function) -> Result<()> {
        for param in &function.params {
            self.collect_type_use(param.ty)?;
        }
        if let Some(result) = function.result {
            self.collect_type_use(result)?;
        }
        Ok(())
    }

    fn collect_type_use(&mut self, ty: Type) -> Result<()> {
        match ty {
            Type::Id(id) => {
                if !self.used_type_ids.insert(id) {
                    return Ok(());
                }
                let kind = self.resolve.types[id].kind.clone();
                match kind {
                    TypeDefKind::Record(record) => {
                        for field in record.fields {
                            self.collect_type_use(field.ty)?;
                        }
                    }
                    TypeDefKind::Flags(_) | TypeDefKind::Enum(_) | TypeDefKind::Resource => {}
                    TypeDefKind::Tuple(tuple) => {
                        for ty in tuple.types {
                            self.collect_type_use(ty)?;
                        }
                    }
                    TypeDefKind::Variant(variant) => {
                        for case in variant.cases {
                            if let Some(ty) = case.ty {
                                self.collect_type_use(ty)?;
                            }
                        }
                    }
                    TypeDefKind::Option(inner)
                    | TypeDefKind::List(inner)
                    | TypeDefKind::Type(inner)
                    | TypeDefKind::FixedLengthList(inner, _) => {
                        self.collect_type_use(inner)?;
                    }
                    TypeDefKind::Result(result) => {
                        if let Some(ok) = result.ok {
                            self.collect_type_use(ok)?;
                        }
                        if let Some(err) = result.err {
                            self.collect_type_use(err)?;
                        }
                    }
                    TypeDefKind::Handle(_) => {}
                    TypeDefKind::Map(_, _) | TypeDefKind::Future(_) | TypeDefKind::Stream(_) => {}
                    TypeDefKind::Unknown => {}
                }
            }
            Type::Bool
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::S8
            | Type::S16
            | Type::S32
            | Type::S64
            | Type::F32
            | Type::F64
            | Type::Char
            | Type::String => {}
            Type::ErrorContext => {
                return Err(Error::new(
                    Span::call_site(),
                    "`error-context` is not supported by telomere-component-bindgen yet",
                ));
            }
        }
        Ok(())
    }

    fn generate(&self) -> Result<TokenStream2> {
        let runtime = &self.runtime_path;
        let internal = self.generate_internal_support();
        let types_module = self.generate_types_module()?;
        let root_aliases = self.generate_type_aliases(&NamespaceLocation::World, ScopeDepth::Root);
        let root_imports = self.generate_root_imports()?;
        let import_modules = self
            .import_interfaces
            .iter()
            .map(|namespace| self.generate_import_interface(namespace))
            .collect::<Result<Vec<_>>>()?;
        let exports = self.generate_exports_root()?;
        let export_modules = self
            .export_interfaces
            .iter()
            .map(|namespace| self.generate_export_interface(namespace))
            .collect::<Result<Vec<_>>>()?;

        Ok(quote! {
            #internal
            #types_module
            #(#root_aliases)*
            #root_imports
            pub mod imports {
                #(#import_modules)*
            }
            #exports
            pub mod exports {
                #(#export_modules)*
            }
            const _: fn(&mut #runtime::Store) = |_store| {};
        })
    }

    fn generate_internal_support(&self) -> TokenStream2 {
        let runtime = &self.runtime_path;
        quote! {
            mod __internal {
                pub fn resolve_defined_type<'a>(
                    ty: &'a #runtime::ir::types::ValType,
                    program: &'a #runtime::ComponentProgram,
                ) -> Result<&'a #runtime::ir::types::Type, #runtime::ComponentError> {
                    match ty {
                        #runtime::ir::types::ValType::Type(type_id) => program
                            .get_type(*type_id)
                            .ok_or_else(|| #runtime::ComponentError::Link("type id not found".to_owned())),
                        #runtime::ir::types::ValType::Primitive(_) => Err(#runtime::ComponentError::Link(
                            "expected defined component type".to_owned(),
                        )),
                    }
                }

                pub fn lower_function_result<T>(
                    value: T,
                ) -> Result<Vec<#runtime::ComponentValue>, #runtime::ComponentError>
                where
                    T: #runtime::ComponentReturn,
                {
                    <T as #runtime::ComponentReturn>::into_component_results(value)
                }

                pub fn lift_function_result<T>(
                    results: Vec<#runtime::ComponentValue>,
                ) -> Result<T, #runtime::ComponentError>
                where
                    T: #runtime::ComponentReturn,
                {
                    <T as #runtime::ComponentReturn>::from_component_results(results)
                }
            }
        }
    }

    fn generate_types_module(&self) -> Result<TokenStream2> {
        let definitions = self
            .named_type_ids
            .iter()
            .map(|type_id| self.generate_named_type(*type_id))
            .collect::<Result<Vec<_>>>()?;

        Ok(quote! {
            mod __types {
                #(#definitions)*
            }

            pub mod types {
                pub use super::__types::*;
            }
        })
    }

    fn generate_named_type(&self, type_id: TypeId) -> Result<TokenStream2> {
        let runtime = &self.runtime_path;
        let typedef = &self.resolve.types[type_id];
        let ident = self.type_unique_idents.get(&type_id).unwrap();

        match &typedef.kind {
            TypeDefKind::Record(record) => {
                let fields = record
                    .fields
                    .iter()
                    .map(|field| {
                        let ident = snake_ident(&field.name);
                        let ty = self.render_internal_type(field.ty)?;
                        Ok(quote!(pub #ident: #ty,))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let lower_fields = record
                    .fields
                    .iter()
                    .map(|field| {
                        let ident = snake_ident(&field.name);
                        let name = LitStr::new(&field.name, Span::call_site());
                        Ok(quote! {
                            (#name.to_owned(), self.#ident.lower_component()?),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let lift_fields = record
                    .fields
                    .iter()
                    .map(|field| {
                        let ident = snake_ident(&field.name);
                        let name = field.name.clone();
                        let ty = self.render_internal_type(field.ty)?;
                        Ok(quote! {
                            let (field_name, field_value) = iter.next().ok_or_else(|| {
                                #runtime::ComponentError::InvalidArgument(
                                    "record field missing".to_owned(),
                                )
                            })?;
                            if field_name != #name {
                                return Err(#runtime::ComponentError::InvalidArgument(format!(
                                    "expected record field `{}`, got `{field_name}`",
                                    #name
                                )));
                            }
                            let #ident = <#ty as #runtime::LiftComponent>::lift_component(field_value)?;
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let matches = record
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(index, field)| {
                        let name = field.name.clone();
                        let ty = self.render_internal_type(field.ty)?;
                        Ok(quote! {
                            if fields[#index].label.0 != #name {
                                return Err(#runtime::ComponentError::Link(
                                    "typed component binding expects record".to_owned(),
                                ));
                            }
                            <#ty as #runtime::LowerComponent>::matches_type(&fields[#index].ty, program)?;
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let field_len = record.fields.len();
                let field_idents = record
                    .fields
                    .iter()
                    .map(|field| snake_ident(&field.name))
                    .collect::<Vec<_>>();
                Ok(quote! {
                    #[derive(Clone, Debug, PartialEq, Eq)]
                    pub struct #ident {
                        #(#fields)*
                    }

                    impl #runtime::LowerComponent for #ident {
                        fn lower_component(self) -> Result<#runtime::ComponentValue, #runtime::ComponentError> {
                            use #runtime::LowerComponent as _;
                            Ok(#runtime::ComponentValue::Record(vec![
                                #(#lower_fields)*
                            ]))
                        }

                        fn matches_type(
                            ty: &#runtime::ir::types::ValType,
                            program: &#runtime::ComponentProgram,
                        ) -> Result<(), #runtime::ComponentError> {
                            let #runtime::ir::types::Type::DefVal(#runtime::ir::types::DefValType::Record(fields)) =
                                super::__internal::resolve_defined_type(ty, program)?
                            else {
                                return Err(#runtime::ComponentError::Link(
                                    "typed component binding expects record".to_owned(),
                                ));
                            };
                            if fields.len() != #field_len {
                                return Err(#runtime::ComponentError::Link(
                                    "typed component binding expects record".to_owned(),
                                ));
                            }
                            #(#matches)*
                            Ok(())
                        }
                    }

                    impl #runtime::LiftComponent for #ident {
                        fn lift_component(value: #runtime::ComponentValue) -> Result<Self, #runtime::ComponentError> {
                            let values = match value {
                                #runtime::ComponentValue::Record(values) => values,
                                other => {
                                    return Err(#runtime::ComponentError::InvalidArgument(format!(
                                        "expected record result, got {other:?}"
                                    )))
                                }
                            };
                            let mut iter = values.into_iter();
                            #(#lift_fields)*
                            if iter.next().is_some() {
                                return Err(#runtime::ComponentError::InvalidArgument(
                                    "record result contains extra fields".to_owned(),
                                ));
                            }
                            Ok(Self {
                                #(#field_idents,)*
                            })
                        }

                        fn matches_type(
                            ty: &#runtime::ir::types::ValType,
                            program: &#runtime::ComponentProgram,
                        ) -> Result<(), #runtime::ComponentError> {
                            <Self as #runtime::LowerComponent>::matches_type(ty, program)
                        }
                    }
                })
            }
            TypeDefKind::Variant(variant) => {
                let variants = variant
                    .cases
                    .iter()
                    .map(|case| {
                        let ident = camel_ident(&case.name);
                        if let Some(ty) = case.ty {
                            let ty = self.render_internal_type(ty)?;
                            Ok(quote!(#ident(#ty),))
                        } else {
                            Ok(quote!(#ident,))
                        }
                    })
                    .collect::<Result<Vec<_>>>()?;
                let lower_arms = variant
                    .cases
                    .iter()
                    .map(|case| {
                        let ident = camel_ident(&case.name);
                        let case_name = LitStr::new(&case.name, Span::call_site());
                        if case.ty.is_some() {
                            Ok(quote! {
                                Self::#ident(value) => #runtime::ComponentValue::Variant {
                                    case: #case_name.to_owned(),
                                    value: Some(Box::new(value.lower_component()?)),
                                },
                            })
                        } else {
                            Ok(quote! {
                                Self::#ident => #runtime::ComponentValue::Variant {
                                    case: #case_name.to_owned(),
                                    value: None,
                                },
                            })
                        }
                    })
                    .collect::<Result<Vec<_>>>()?;
                let lift_arms = variant
                    .cases
                    .iter()
                    .map(|case| {
                        let ident = camel_ident(&case.name);
                        let case_name = case.name.clone();
                        if let Some(ty) = case.ty {
                            let ty = self.render_internal_type(ty)?;
                            Ok(quote! {
                                #case_name => {
                                    let value = value.ok_or_else(|| {
                                        #runtime::ComponentError::InvalidArgument(
                                            "variant payload missing".to_owned(),
                                        )
                                    })?;
                                    Ok(Self::#ident(<#ty as #runtime::LiftComponent>::lift_component(*value)?))
                                }
                            })
                        } else {
                            Ok(quote! {
                                #case_name => {
                                    if value.is_some() {
                                        return Err(#runtime::ComponentError::InvalidArgument(
                                            "unexpected variant payload".to_owned(),
                                        ));
                                    }
                                    Ok(Self::#ident)
                                }
                            })
                        }
                    })
                    .collect::<Result<Vec<_>>>()?;
                let matches = variant
                    .cases
                    .iter()
                    .enumerate()
                    .map(|(index, case)| {
                        let case_name = case.name.clone();
                        if let Some(ty) = case.ty {
                            let ty = self.render_internal_type(ty)?;
                            Ok(quote! {
                                if cases[#index].label.0 != #case_name {
                                    return Err(#runtime::ComponentError::Link(
                                        "typed component binding expects variant".to_owned(),
                                    ));
                                }
                                let payload = cases[#index].ty.as_ref().ok_or_else(|| {
                                    #runtime::ComponentError::Link(
                                        "typed component binding expects variant payload".to_owned(),
                                    )
                                })?;
                                <#ty as #runtime::LowerComponent>::matches_type(payload, program)?;
                            })
                        } else {
                            Ok(quote! {
                                if cases[#index].label.0 != #case_name || cases[#index].ty.is_some() {
                                    return Err(#runtime::ComponentError::Link(
                                        "typed component binding expects variant".to_owned(),
                                    ));
                                }
                            })
                        }
                    })
                    .collect::<Result<Vec<_>>>()?;
                let case_len = variant.cases.len();
                Ok(quote! {
                    #[derive(Clone, Debug, PartialEq, Eq)]
                    pub enum #ident {
                        #(#variants)*
                    }

                    impl #runtime::LowerComponent for #ident {
                        fn lower_component(self) -> Result<#runtime::ComponentValue, #runtime::ComponentError> {
                            use #runtime::LowerComponent as _;
                            Ok(match self {
                                #(#lower_arms)*
                            })
                        }

                        fn matches_type(
                            ty: &#runtime::ir::types::ValType,
                            program: &#runtime::ComponentProgram,
                        ) -> Result<(), #runtime::ComponentError> {
                            let #runtime::ir::types::Type::DefVal(#runtime::ir::types::DefValType::Variant(cases)) =
                                super::__internal::resolve_defined_type(ty, program)?
                            else {
                                return Err(#runtime::ComponentError::Link(
                                    "typed component binding expects variant".to_owned(),
                                ));
                            };
                            if cases.len() != #case_len {
                                return Err(#runtime::ComponentError::Link(
                                    "typed component binding expects variant".to_owned(),
                                ));
                            }
                            #(#matches)*
                            Ok(())
                        }
                    }

                    impl #runtime::LiftComponent for #ident {
                        fn lift_component(value: #runtime::ComponentValue) -> Result<Self, #runtime::ComponentError> {
                            let (case, value) = match value {
                                #runtime::ComponentValue::Variant { case, value } => (case, value),
                                other => {
                                    return Err(#runtime::ComponentError::InvalidArgument(format!(
                                        "expected variant result, got {other:?}"
                                    )))
                                }
                            };
                            match case.as_str() {
                                #(#lift_arms,)*
                                _ => Err(#runtime::ComponentError::InvalidArgument(format!(
                                    "unknown variant case `{case}`"
                                ))),
                            }
                        }

                        fn matches_type(
                            ty: &#runtime::ir::types::ValType,
                            program: &#runtime::ComponentProgram,
                        ) -> Result<(), #runtime::ComponentError> {
                            <Self as #runtime::LowerComponent>::matches_type(ty, program)
                        }
                    }
                })
            }
            TypeDefKind::Enum(enum_) => {
                let cases = enum_
                    .cases
                    .iter()
                    .map(|case| {
                        let ident = camel_ident(&case.name);
                        quote!(#ident,)
                    })
                    .collect::<Vec<_>>();
                let lower_arms = enum_
                    .cases
                    .iter()
                    .map(|case| {
                        let ident = camel_ident(&case.name);
                        let case_name = LitStr::new(&case.name, Span::call_site());
                        quote!(Self::#ident => #runtime::ComponentValue::Enum(#case_name.to_owned()),)
                    })
                    .collect::<Vec<_>>();
                let lift_arms = enum_
                    .cases
                    .iter()
                    .map(|case| {
                        let ident = camel_ident(&case.name);
                        let case_name = case.name.clone();
                        quote!(#case_name => Ok(Self::#ident),)
                    })
                    .collect::<Vec<_>>();
                let matches = enum_
                    .cases
                    .iter()
                    .enumerate()
                    .map(|(index, case)| {
                        let case_name = case.name.clone();
                        quote! {
                            if cases[#index].label.0 != #case_name || cases[#index].ty.is_some() {
                                return Err(#runtime::ComponentError::Link(
                                    "typed component binding expects enum".to_owned(),
                                ));
                            }
                        }
                    })
                    .collect::<Vec<_>>();
                let case_len = enum_.cases.len();
                Ok(quote! {
                    #[derive(Clone, Debug, PartialEq, Eq)]
                    pub enum #ident {
                        #(#cases)*
                    }

                    impl #runtime::LowerComponent for #ident {
                        fn lower_component(self) -> Result<#runtime::ComponentValue, #runtime::ComponentError> {
                            Ok(match self {
                                #(#lower_arms)*
                            })
                        }

                        fn matches_type(
                            ty: &#runtime::ir::types::ValType,
                            program: &#runtime::ComponentProgram,
                        ) -> Result<(), #runtime::ComponentError> {
                            let #runtime::ir::types::Type::DefVal(#runtime::ir::types::DefValType::Variant(cases)) =
                                super::__internal::resolve_defined_type(ty, program)?
                            else {
                                return Err(#runtime::ComponentError::Link(
                                    "typed component binding expects enum".to_owned(),
                                ));
                            };
                            if cases.len() != #case_len {
                                return Err(#runtime::ComponentError::Link(
                                    "typed component binding expects enum".to_owned(),
                                ));
                            }
                            #(#matches)*
                            Ok(())
                        }
                    }

                    impl #runtime::LiftComponent for #ident {
                        fn lift_component(value: #runtime::ComponentValue) -> Result<Self, #runtime::ComponentError> {
                            let case = match value {
                                #runtime::ComponentValue::Enum(case) => case,
                                other => {
                                    return Err(#runtime::ComponentError::InvalidArgument(format!(
                                        "expected enum result, got {other:?}"
                                    )))
                                }
                            };
                            match case.as_str() {
                                #(#lift_arms)*
                                _ => Err(#runtime::ComponentError::InvalidArgument(format!(
                                    "unknown enum case `{case}`"
                                ))),
                            }
                        }

                        fn matches_type(
                            ty: &#runtime::ir::types::ValType,
                            program: &#runtime::ComponentProgram,
                        ) -> Result<(), #runtime::ComponentError> {
                            <Self as #runtime::LowerComponent>::matches_type(ty, program)
                        }
                    }
                })
            }
            TypeDefKind::Flags(flags) => {
                let flag_fields = flags
                    .flags
                    .iter()
                    .map(|flag| {
                        let ident = snake_ident(&flag.name);
                        quote!(pub #ident: bool,)
                    })
                    .collect::<Vec<_>>();
                let lower_flags = flags
                    .flags
                    .iter()
                    .map(|flag| {
                        let ident = snake_ident(&flag.name);
                        let name = LitStr::new(&flag.name, Span::call_site());
                        quote! {
                            if self.#ident {
                                selected.push(#name.to_owned());
                            }
                        }
                    })
                    .collect::<Vec<_>>();
                let lift_flags = flags
                    .flags
                    .iter()
                    .map(|flag| {
                        let ident = snake_ident(&flag.name);
                        quote!(#ident: false,)
                    })
                    .collect::<Vec<_>>();
                let flag_arms = flags
                    .flags
                    .iter()
                    .map(|flag| {
                        let name = flag.name.clone();
                        let ident = snake_ident(&flag.name);
                        quote! {
                            #name => {
                                flags.#ident = true;
                            }
                        }
                    })
                    .collect::<Vec<_>>();
                let matches = flags
                    .flags
                    .iter()
                    .enumerate()
                    .map(|(index, flag)| {
                        let name = flag.name.clone();
                        quote! {
                            if labels[#index].0 != #name {
                                return Err(#runtime::ComponentError::Link(
                                    "typed component binding expects flags".to_owned(),
                                ));
                            }
                        }
                    })
                    .collect::<Vec<_>>();
                let flag_len = flags.flags.len();
                Ok(quote! {
                    #[derive(Clone, Debug, Default, PartialEq, Eq)]
                    pub struct #ident {
                        #(#flag_fields)*
                    }

                    impl #runtime::LowerComponent for #ident {
                        fn lower_component(self) -> Result<#runtime::ComponentValue, #runtime::ComponentError> {
                            let mut selected = Vec::new();
                            #(#lower_flags)*
                            Ok(#runtime::ComponentValue::Flags(selected))
                        }

                        fn matches_type(
                            ty: &#runtime::ir::types::ValType,
                            program: &#runtime::ComponentProgram,
                        ) -> Result<(), #runtime::ComponentError> {
                            let #runtime::ir::types::Type::DefVal(#runtime::ir::types::DefValType::Flags(labels)) =
                                super::__internal::resolve_defined_type(ty, program)?
                            else {
                                return Err(#runtime::ComponentError::Link(
                                    "typed component binding expects flags".to_owned(),
                                ));
                            };
                            if labels.len() != #flag_len {
                                return Err(#runtime::ComponentError::Link(
                                    "typed component binding expects flags".to_owned(),
                                ));
                            }
                            #(#matches)*
                            Ok(())
                        }
                    }

                    impl #runtime::LiftComponent for #ident {
                        fn lift_component(value: #runtime::ComponentValue) -> Result<Self, #runtime::ComponentError> {
                            let values = match value {
                                #runtime::ComponentValue::Flags(values) => values,
                                other => {
                                    return Err(#runtime::ComponentError::InvalidArgument(format!(
                                        "expected flags result, got {other:?}"
                                    )))
                                }
                            };
                            let mut flags = Self {
                                #(#lift_flags)*
                            };
                            for name in values {
                                match name.as_str() {
                                    #(#flag_arms,)*
                                    _ => {
                                        return Err(#runtime::ComponentError::InvalidArgument(format!(
                                            "unknown flag `{name}`"
                                        )));
                                    }
                                }
                            }
                            Ok(flags)
                        }

                        fn matches_type(
                            ty: &#runtime::ir::types::ValType,
                            program: &#runtime::ComponentProgram,
                        ) -> Result<(), #runtime::ComponentError> {
                            <Self as #runtime::LowerComponent>::matches_type(ty, program)
                        }
                    }
                })
            }
            TypeDefKind::Tuple(tuple) => {
                let types = tuple
                    .types
                    .iter()
                    .map(|ty| self.render_internal_type(*ty))
                    .collect::<Result<Vec<_>>>()?;
                Ok(quote!(pub type #ident = (#(#types,)*);))
            }
            TypeDefKind::Option(inner) => {
                let ty = self.render_internal_type(*inner)?;
                Ok(quote!(pub type #ident = Option<#ty>;))
            }
            TypeDefKind::Result(result) => {
                let (Some(ok), Some(err)) = (result.ok, result.err) else {
                    return Err(Error::new(
                        Span::call_site(),
                        "result types without both `ok` and `err` payloads are not supported yet",
                    ));
                };
                let ok = self.render_internal_type(ok)?;
                let err = self.render_internal_type(err)?;
                Ok(quote!(pub type #ident = Result<#ok, #err>;))
            }
            TypeDefKind::List(inner) | TypeDefKind::FixedLengthList(inner, _) => {
                let ty = self.render_internal_type(*inner)?;
                Ok(quote!(pub type #ident = Vec<#ty>;))
            }
            TypeDefKind::Type(inner) => {
                let ty = self.render_internal_type(*inner)?;
                Ok(quote!(pub type #ident = #ty;))
            }
            TypeDefKind::Resource | TypeDefKind::Handle(_) => Err(Error::new(
                Span::call_site(),
                format!(
                    "resource type `{}` is not supported by telomere-component-bindgen yet",
                    typedef.name.as_deref().unwrap_or("<anonymous>")
                ),
            )),
            TypeDefKind::Map(_, _)
            | TypeDefKind::Future(_)
            | TypeDefKind::Stream(_)
            | TypeDefKind::Unknown => Err(Error::new(
                Span::call_site(),
                format!(
                    "type kind `{}` is not supported by telomere-component-bindgen yet",
                    typedef.kind.as_str()
                ),
            )),
        }
    }

    fn render_internal_type(&self, ty: Type) -> Result<TokenStream2> {
        match ty {
            Type::Bool => Ok(quote!(bool)),
            Type::U8 => Ok(quote!(u8)),
            Type::U16 => Ok(quote!(u16)),
            Type::U32 => Ok(quote!(u32)),
            Type::U64 => Ok(quote!(u64)),
            Type::S8 => Ok(quote!(i8)),
            Type::S16 => Ok(quote!(i16)),
            Type::S32 => Ok(quote!(i32)),
            Type::S64 => Ok(quote!(i64)),
            Type::F32 => Ok(quote!(f32)),
            Type::F64 => Ok(quote!(f64)),
            Type::Char => Ok(quote!(char)),
            Type::String => Ok(quote!(String)),
            Type::ErrorContext => Err(Error::new(
                Span::call_site(),
                "`error-context` is not supported by telomere-component-bindgen yet",
            )),
            Type::Id(id) => {
                let typedef = &self.resolve.types[id];
                if typedef.name.is_some() {
                    let ident = self.type_unique_idents.get(&id).unwrap();
                    return Ok(quote!(#ident));
                }
                match &typedef.kind {
                    TypeDefKind::Tuple(tuple) => {
                        let types = tuple
                            .types
                            .iter()
                            .map(|ty| self.render_internal_type(*ty))
                            .collect::<Result<Vec<_>>>()?;
                        Ok(quote!((#(#types,)*)))
                    }
                    TypeDefKind::Option(inner) => {
                        let ty = self.render_internal_type(*inner)?;
                        Ok(quote!(Option<#ty>))
                    }
                    TypeDefKind::Result(result) => {
                        let (Some(ok), Some(err)) = (result.ok, result.err) else {
                            return Err(Error::new(
                                Span::call_site(),
                                "result types without both `ok` and `err` payloads are not supported yet",
                            ));
                        };
                        let ok = self.render_internal_type(ok)?;
                        let err = self.render_internal_type(err)?;
                        Ok(quote!(Result<#ok, #err>))
                    }
                    TypeDefKind::List(inner) | TypeDefKind::FixedLengthList(inner, _) => {
                        let ty = self.render_internal_type(*inner)?;
                        Ok(quote!(Vec<#ty>))
                    }
                    TypeDefKind::Type(inner) => self.render_internal_type(*inner),
                    TypeDefKind::Record(_)
                    | TypeDefKind::Variant(_)
                    | TypeDefKind::Enum(_)
                    | TypeDefKind::Flags(_) => Err(Error::new(
                        Span::call_site(),
                        "anonymous composite type is not supported by telomere-component-bindgen",
                    )),
                    TypeDefKind::Resource
                    | TypeDefKind::Handle(_)
                    | TypeDefKind::Map(_, _)
                    | TypeDefKind::Future(_)
                    | TypeDefKind::Stream(_)
                    | TypeDefKind::Unknown => Err(Error::new(
                        Span::call_site(),
                        format!(
                            "type kind `{}` is not supported by telomere-component-bindgen yet",
                            typedef.kind.as_str()
                        ),
                    )),
                }
            }
        }
    }

    fn render_public_type(&self, ty: Type, depth: ScopeDepth) -> Result<TokenStream2> {
        match ty {
            Type::Bool => Ok(quote!(bool)),
            Type::U8 => Ok(quote!(u8)),
            Type::U16 => Ok(quote!(u16)),
            Type::U32 => Ok(quote!(u32)),
            Type::U64 => Ok(quote!(u64)),
            Type::S8 => Ok(quote!(i8)),
            Type::S16 => Ok(quote!(i16)),
            Type::S32 => Ok(quote!(i32)),
            Type::S64 => Ok(quote!(i64)),
            Type::F32 => Ok(quote!(f32)),
            Type::F64 => Ok(quote!(f64)),
            Type::Char => Ok(quote!(char)),
            Type::String => Ok(quote!(String)),
            Type::ErrorContext => Err(Error::new(
                Span::call_site(),
                "`error-context` is not supported by telomere-component-bindgen yet",
            )),
            Type::Id(id) => {
                let typedef = &self.resolve.types[id];
                if typedef.name.is_some() {
                    let prefix = match depth {
                        ScopeDepth::Root => quote!(types),
                        ScopeDepth::Nested => quote!(super::super::types),
                    };
                    let ident = self.type_unique_idents.get(&id).unwrap();
                    return Ok(quote!(#prefix::#ident));
                }
                match &typedef.kind {
                    TypeDefKind::Tuple(tuple) => {
                        let types = tuple
                            .types
                            .iter()
                            .map(|ty| self.render_public_type(*ty, depth))
                            .collect::<Result<Vec<_>>>()?;
                        Ok(quote!((#(#types,)*)))
                    }
                    TypeDefKind::Option(inner) => {
                        let ty = self.render_public_type(*inner, depth)?;
                        Ok(quote!(Option<#ty>))
                    }
                    TypeDefKind::Result(result) => {
                        let (Some(ok), Some(err)) = (result.ok, result.err) else {
                            return Err(Error::new(
                                Span::call_site(),
                                "result types without both `ok` and `err` payloads are not supported yet",
                            ));
                        };
                        let ok = self.render_public_type(ok, depth)?;
                        let err = self.render_public_type(err, depth)?;
                        Ok(quote!(Result<#ok, #err>))
                    }
                    TypeDefKind::List(inner) | TypeDefKind::FixedLengthList(inner, _) => {
                        let ty = self.render_public_type(*inner, depth)?;
                        Ok(quote!(Vec<#ty>))
                    }
                    TypeDefKind::Type(inner) => self.render_public_type(*inner, depth),
                    TypeDefKind::Record(_)
                    | TypeDefKind::Variant(_)
                    | TypeDefKind::Enum(_)
                    | TypeDefKind::Flags(_) => Err(Error::new(
                        Span::call_site(),
                        "anonymous composite type is not supported by telomere-component-bindgen",
                    )),
                    TypeDefKind::Resource
                    | TypeDefKind::Handle(_)
                    | TypeDefKind::Map(_, _)
                    | TypeDefKind::Future(_)
                    | TypeDefKind::Stream(_)
                    | TypeDefKind::Unknown => Err(Error::new(
                        Span::call_site(),
                        format!(
                            "type kind `{}` is not supported by telomere-component-bindgen yet",
                            typedef.kind.as_str()
                        ),
                    )),
                }
            }
        }
    }

    fn generate_type_aliases(
        &self,
        location: &NamespaceLocation,
        depth: ScopeDepth,
    ) -> Vec<TokenStream2> {
        let prefix = match depth {
            ScopeDepth::Root => quote!(types),
            ScopeDepth::Nested => quote!(super::super::types),
        };
        let Some(type_ids) = self.type_locations.get(location) else {
            return Vec::new();
        };
        let mut type_ids = type_ids.clone();
        type_ids.sort_by_key(|type_id| type_id.index());
        type_ids
            .into_iter()
            .filter_map(|type_id| {
                self.type_alias_idents.get(&type_id).map(|alias| {
                    let unique = self.type_unique_idents.get(&type_id).unwrap();
                    quote!(pub use #prefix::#unique as #alias;)
                })
            })
            .collect()
    }

    fn generate_root_imports(&self) -> Result<TokenStream2> {
        let runtime = &self.runtime_path;
        if self.direct_imports.is_empty() {
            return Ok(TokenStream2::new());
        }
        let methods = self
            .direct_imports
            .iter()
            .map(|binding| self.generate_import_trait_method(binding, ScopeDepth::Root))
            .collect::<Result<Vec<_>>>()?;
        let registrations = self
            .direct_imports
            .iter()
            .map(|binding| self.generate_root_import_registration(binding))
            .collect::<Result<Vec<_>>>()?;
        Ok(quote! {
            pub trait Imports {
                #(#methods)*
            }

            pub fn add_root_imports_to_linker<T>(
                linker: &mut #runtime::ComponentLinker,
                host: ::std::rc::Rc<T>,
            )
            where
                T: Imports + 'static,
            {
                #(#registrations)*
            }
        })
    }

    fn generate_import_interface(&self, namespace: &InterfaceNamespace) -> Result<TokenStream2> {
        let runtime = &self.runtime_path;
        let module_ident = &namespace.module_ident;
        let aliases = self.generate_type_aliases(&namespace.location, ScopeDepth::Nested);
        let methods = namespace
            .functions
            .iter()
            .map(|binding| self.generate_import_trait_method(binding, ScopeDepth::Nested))
            .collect::<Result<Vec<_>>>()?;
        let registrations = namespace
            .functions
            .iter()
            .map(|binding| self.generate_instance_import_registration(binding))
            .collect::<Result<Vec<_>>>()?;
        let wit_name = LitStr::new(&namespace.wit_name, Span::call_site());
        Ok(quote! {
            pub mod #module_ident {
                #(#aliases)*

                pub trait Host {
                    #(#methods)*
                }

                pub fn add_to_linker<T>(
                    linker: &mut #runtime::ComponentLinker,
                    host: ::std::rc::Rc<T>,
                )
                where
                    T: Host + 'static,
                {
                    let mut instance = #runtime::ComponentLinkerInstance::new();
                    #(#registrations)*
                    linker.register_import_instance(#wit_name, instance);
                }
            }
        })
    }

    fn generate_exports_root(&self) -> Result<TokenStream2> {
        let runtime = &self.runtime_path;
        let direct_methods = self
            .direct_exports
            .iter()
            .map(|binding| self.generate_export_method(binding, ScopeDepth::Root))
            .collect::<Result<Vec<_>>>()?;
        let interface_accessors = self
            .export_interfaces
            .iter()
            .map(|namespace| {
                let method_ident = &namespace.module_ident;
                let module_ident = &namespace.module_ident;
                let wit_name = LitStr::new(&namespace.wit_name, Span::call_site());
                Ok(quote! {
                    pub fn #method_ident(&self) -> exports::#module_ident::Exports {
                        exports::#module_ident::Exports::new(self.instance.get_instance(#wit_name))
                    }
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(quote! {
            #[derive(Clone)]
            pub struct Exports {
                instance: #runtime::ComponentExports,
            }

            impl Exports {
                pub fn new(instance: #runtime::ComponentInstance) -> Self {
                    Self {
                        instance: instance.exports(),
                    }
                }

                #(#interface_accessors)*
                #(#direct_methods)*
            }
        })
    }

    fn generate_export_interface(&self, namespace: &InterfaceNamespace) -> Result<TokenStream2> {
        let runtime = &self.runtime_path;
        let module_ident = &namespace.module_ident;
        let aliases = self.generate_type_aliases(&namespace.location, ScopeDepth::Nested);
        let methods = namespace
            .functions
            .iter()
            .map(|binding| self.generate_export_method(binding, ScopeDepth::Nested))
            .collect::<Result<Vec<_>>>()?;
        Ok(quote! {
            pub mod #module_ident {
                #(#aliases)*

                #[derive(Clone)]
                pub struct Exports {
                    instance: #runtime::ComponentExports,
                }

                impl Exports {
                    pub(crate) fn new(instance: #runtime::ComponentExports) -> Self {
                        Self { instance }
                    }

                    #(#methods)*
                }
            }
        })
    }

    fn generate_import_trait_method(
        &self,
        binding: &FunctionBinding,
        depth: ScopeDepth,
    ) -> Result<TokenStream2> {
        let runtime = &self.runtime_path;
        let method_ident = &binding.method_ident;
        let params = binding
            .function
            .params
            .iter()
            .map(|param| {
                let ident = snake_ident(&param.name);
                let ty = self.render_public_type(param.ty, depth)?;
                Ok(quote!(#ident: #ty))
            })
            .collect::<Result<Vec<_>>>()?;
        let result = binding
            .function
            .result
            .map(|ty| self.render_public_type(ty, depth))
            .transpose()?
            .unwrap_or_else(|| quote!(()));
        Ok(quote! {
            fn #method_ident(
                &self,
                store: &mut #runtime::Store,
                #(#params),*
            ) -> Result<#result, #runtime::ComponentError>;
        })
    }

    fn generate_root_import_registration(&self, binding: &FunctionBinding) -> Result<TokenStream2> {
        let wit_name = LitStr::new(&binding.wit_name, Span::call_site());
        let method_ident = &binding.method_ident;
        let arg_prep = self.generate_argument_lifts(binding, ScopeDepth::Root)?;
        let arg_names = binding
            .function
            .params
            .iter()
            .map(|param| snake_ident(&param.name))
            .collect::<Vec<_>>();
        let result = binding
            .function
            .result
            .map(|ty| self.render_public_type(ty, ScopeDepth::Root))
            .transpose()?
            .unwrap_or_else(|| quote!(()));
        Ok(quote! {
            linker.register_import(#wit_name, {
                let host = ::std::rc::Rc::clone(&host);
                move |store, args| {
                    #arg_prep
                    let result: #result = host.#method_ident(store, #(#arg_names),*)?;
                    __internal::lower_function_result::<#result>(result)
                }
            });
        })
    }

    fn generate_instance_import_registration(
        &self,
        binding: &FunctionBinding,
    ) -> Result<TokenStream2> {
        let wit_name = LitStr::new(&binding.wit_name, Span::call_site());
        let method_ident = &binding.method_ident;
        let arg_prep = self.generate_argument_lifts(binding, ScopeDepth::Nested)?;
        let arg_names = binding
            .function
            .params
            .iter()
            .map(|param| snake_ident(&param.name))
            .collect::<Vec<_>>();
        let result = binding
            .function
            .result
            .map(|ty| self.render_public_type(ty, ScopeDepth::Nested))
            .transpose()?
            .unwrap_or_else(|| quote!(()));
        Ok(quote! {
            instance.register_func(#wit_name, {
                let host = ::std::rc::Rc::clone(&host);
                move |store, args| {
                    #arg_prep
                    let result: #result = host.#method_ident(store, #(#arg_names),*)?;
                    super::super::__internal::lower_function_result::<#result>(result)
                }
            });
        })
    }

    fn generate_export_method(
        &self,
        binding: &FunctionBinding,
        depth: ScopeDepth,
    ) -> Result<TokenStream2> {
        let runtime = &self.runtime_path;
        let internal = match depth {
            ScopeDepth::Root => quote!(__internal),
            ScopeDepth::Nested => quote!(super::super::__internal),
        };
        let method_ident = &binding.method_ident;
        let wit_name = LitStr::new(&binding.wit_name, Span::call_site());
        let params = binding
            .function
            .params
            .iter()
            .map(|param| {
                let ident = snake_ident(&param.name);
                let ty = self.render_public_type(param.ty, depth)?;
                Ok(quote!(#ident: #ty))
            })
            .collect::<Result<Vec<_>>>()?;
        let arg_pushes = binding
            .function
            .params
            .iter()
            .map(|param| {
                let ident = snake_ident(&param.name);
                Ok(quote!(args.push(#ident.lower_component()?);))
            })
            .collect::<Result<Vec<_>>>()?;
        let result = binding
            .function
            .result
            .map(|ty| self.render_public_type(ty, depth))
            .transpose()?
            .unwrap_or_else(|| quote!(()));
        let arg_len = binding.function.params.len();
        Ok(quote! {
            pub async fn #method_ident(
                &self,
                store: &mut #runtime::Store,
                #(#params),*
            ) -> Result<#result, #runtime::ComponentError> {
                use #runtime::LowerComponent as _;
                let mut args = Vec::with_capacity(#arg_len);
                #(#arg_pushes)*
                let results = self.instance.call(store, #wit_name, &args).await?;
                #internal::lift_function_result::<#result>(results)
            }
        })
    }

    fn generate_argument_lifts(
        &self,
        binding: &FunctionBinding,
        depth: ScopeDepth,
    ) -> Result<TokenStream2> {
        let runtime = &self.runtime_path;
        let expected = binding.function.params.len();
        let lifts = binding
            .function
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                let ident = snake_ident(&param.name);
                let ty = self.render_public_type(param.ty, depth)?;
                Ok(quote! {
                    let #ident = <#ty as #runtime::LiftComponent>::lift_component(
                        args[#index].clone(),
                    )?;
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(quote! {
            if args.len() != #expected {
                return Err(#runtime::ComponentError::InvalidArgument(format!(
                    "function expects {} component arguments, got {}",
                    #expected,
                    args.len()
                )));
            }
            #(#lifts)*
        })
    }
}

fn last_segment(text: &str) -> &str {
    text.rsplit('/')
        .next()
        .unwrap_or(text)
        .split('@')
        .next()
        .unwrap_or(text)
}

fn camel_ident(text: &str) -> Ident {
    let mut candidate = sanitize(text).to_upper_camel_case();
    if candidate.is_empty() {
        candidate = "Generated".to_owned();
    }
    if candidate.chars().next().unwrap().is_ascii_digit() {
        candidate.insert(0, '_');
    }
    if is_reserved(&candidate) {
        candidate.push('_');
    }
    Ident::new(&candidate, Span::call_site())
}

fn snake_ident(text: &str) -> Ident {
    let mut candidate = sanitize(text).to_snake_case();
    if candidate.is_empty() {
        candidate = "generated".to_owned();
    }
    if candidate.chars().next().unwrap().is_ascii_digit() {
        candidate.insert(0, '_');
    }
    if is_reserved(&candidate) {
        candidate.push('_');
    }
    Ident::new(&candidate, Span::call_site())
}

fn sanitize(text: &str) -> String {
    let mut out = String::new();
    let mut last_was_sep = false;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('_');
            last_was_sep = true;
        }
    }
    out.trim_matches('_').to_owned()
}

fn is_reserved(text: &str) -> bool {
    matches!(
        text,
        "Self"
            | "as"
            | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
    )
}
