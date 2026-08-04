use heck::{ToSnakeCase, ToUpperCamelCase};
use proc_macro2::{Ident, Span, TokenStream as TokenStream2};
use proc_macro_crate::{crate_name, FoundCrate};
use quote::quote;
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;
use syn::parse::{Parse, ParseStream};
use syn::{braced, bracketed, parse_quote, Error, LitBool, LitStr, Path, Result, Token};
use wit_parser::{
    Function, FunctionKind, Handle, InterfaceId, PackageId, Resolve, Type, TypeDefKind, TypeId,
    TypeOwner, WorldId, WorldItem,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum HostMode {
    Sync,
    Async,
    Both,
}

impl HostMode {
    fn includes_sync(self) -> bool {
        matches!(self, Self::Sync | Self::Both)
    }

    fn includes_async(self) -> bool {
        matches!(self, Self::Async | Self::Both)
    }
}

#[derive(Clone)]
struct AdoptMapping {
    prefix: LitStr,
    path: Path,
}

pub struct BindgenInput {
    inline: Option<LitStr>,
    path: Option<LitStr>,
    deps: Vec<LitStr>,
    world: LitStr,
    module: Option<LitStr>,
    host_mode: HostMode,
    adopt: Vec<AdoptMapping>,
    strip_interface_version: bool,
}

impl Parse for BindgenInput {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let content;
        braced!(content in input);

        let mut inline = None;
        let mut path = None;
        let mut deps = Vec::new();
        let mut world = None;
        let mut module = None;
        let mut host_mode = None;
        let mut adopt = Vec::new();
        let mut strip_interface_version = None;

        while !content.is_empty() {
            let key = content.parse::<Ident>()?;
            content.parse::<Token![:]>()?;
            match key.to_string().as_str() {
                "inline" => {
                    let value = content.parse::<LitStr>()?;
                    set_once(&mut inline, value, "inline")?;
                }
                "path" => {
                    let value = content.parse::<LitStr>()?;
                    set_once(&mut path, value, "path")?;
                }
                "deps" => {
                    let deps_content;
                    bracketed!(deps_content in content);
                    while !deps_content.is_empty() {
                        deps.push(deps_content.parse::<LitStr>()?);
                        if deps_content.is_empty() {
                            break;
                        }
                        deps_content.parse::<Token![,]>()?;
                    }
                }
                "world" => {
                    let value = content.parse::<LitStr>()?;
                    set_once(&mut world, value, "world")?;
                }
                "module" => {
                    let value = content.parse::<LitStr>()?;
                    set_once(&mut module, value, "module")?;
                }
                "host_mode" => {
                    let value = content.parse::<LitStr>()?;
                    let parsed = match value.value().as_str() {
                        "sync" => HostMode::Sync,
                        "async" => HostMode::Async,
                        "both" => HostMode::Both,
                        other => {
                            return Err(Error::new(
                                value.span(),
                                format!(
                                    "unsupported `host_mode` value `{other}`; expected `sync`, `async`, or `both`"
                                ),
                            ))
                        }
                    };
                    if host_mode.replace(parsed).is_some() {
                        return Err(Error::new(
                            value.span(),
                            "`host_mode` specified more than once",
                        ));
                    }
                }
                "adopt" => {
                    let adopt_content;
                    braced!(adopt_content in content);
                    while !adopt_content.is_empty() {
                        let prefix = adopt_content.parse::<LitStr>()?;
                        adopt_content.parse::<Token![=>]>()?;
                        let path = adopt_content.parse::<Path>()?;
                        adopt.push(AdoptMapping { prefix, path });
                        if adopt_content.is_empty() {
                            break;
                        }
                        adopt_content.parse::<Token![,]>()?;
                    }
                }
                "strip_interface_version" => {
                    let value = content.parse::<LitBool>()?;
                    if strip_interface_version.replace(value.value).is_some() {
                        return Err(Error::new(
                            value.span(),
                            "`strip_interface_version` specified more than once",
                        ));
                    }
                }
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
            deps,
            world,
            module,
            host_mode: host_mode.unwrap_or(HostMode::Sync),
            adopt,
            strip_interface_version: strip_interface_version.unwrap_or(false),
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
    let bindgen_path: syn::Path = parse_quote!(#runtime_path::__bindgen);
    let module_ident = input
        .module
        .as_ref()
        .map(|value| snake_ident(&value.value()))
        .unwrap_or_else(|| snake_ident(last_segment(&input.world.value())));

    let (mut resolve, world_id) = load_world(&input)?;
    resolve.generate_nominal_type_ids(world_id);

    let generator = Generator::new(
        resolve,
        world_id,
        runtime_path,
        bindgen_path,
        input.host_mode,
        input.adopt,
        input.strip_interface_version,
    )?;
    let body = generator.generate()?;
    Ok(quote! {
        #[allow(missing_docs)]
        pub mod #module_ident {
            #body
        }
    })
}

fn load_world(input: &BindgenInput) -> Result<(Resolve, WorldId)> {
    let mut resolve = Resolve::default();
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .map_err(|error| Error::new(Span::call_site(), error.to_string()))?;
    let mut packages = Vec::new();
    for dep in &input.deps {
        let absolute = manifest_dir.join(dep.value());
        let (package, _) = resolve
            .push_path(&absolute)
            .map_err(|error| Error::new(dep.span(), error.to_string()))?;
        packages.push(package);
    }
    if let Some(inline) = &input.inline {
        packages.push(
            resolve
                .push_source("<inline>.wit", &inline.value())
                .map_err(|error| Error::new(inline.span(), error.to_string()))?,
        );
    } else {
        let path = input.path.as_ref().unwrap();
        let absolute = manifest_dir.join(path.value());
        let (package, _) = resolve
            .push_path(&absolute)
            .map_err(|error| Error::new(path.span(), error.to_string()))?;
        packages.push(package);
    }

    let world_id = resolve
        .select_world(&packages, Some(&input.world.value()))
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
    adopt_path: Option<Path>,
}

struct Generator {
    resolve: Resolve,
    world_id: WorldId,
    runtime_path: syn::Path,
    bindgen_path: syn::Path,
    host_mode: HostMode,
    adopt_mappings: Vec<AdoptMapping>,
    strip_interface_version: bool,
    direct_imports: Vec<FunctionBinding>,
    direct_exports: Vec<FunctionBinding>,
    import_interfaces: Vec<InterfaceNamespace>,
    export_interfaces: Vec<InterfaceNamespace>,
    used_type_ids: HashSet<TypeId>,
    named_type_ids: Vec<TypeId>,
    type_unique_idents: HashMap<TypeId, Ident>,
    type_alias_idents: HashMap<TypeId, Ident>,
    resource_marker_idents: HashMap<TypeId, Ident>,
    resource_borrow_alias_idents: HashMap<TypeId, Ident>,
    type_locations: HashMap<NamespaceLocation, Vec<TypeId>>,
}

impl Generator {
    fn new(
        resolve: Resolve,
        world_id: WorldId,
        runtime_path: syn::Path,
        bindgen_path: syn::Path,
        host_mode: HostMode,
        adopt_mappings: Vec<AdoptMapping>,
        strip_interface_version: bool,
    ) -> Result<Self> {
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
            bindgen_path,
            host_mode,
            adopt_mappings,
            strip_interface_version,
            direct_imports: Vec::new(),
            direct_exports: Vec::new(),
            import_interfaces: Vec::new(),
            export_interfaces: Vec::new(),
            used_type_ids: HashSet::new(),
            named_type_ids: Vec::new(),
            type_unique_idents: HashMap::new(),
            type_alias_idents: HashMap::new(),
            resource_marker_idents: HashMap::new(),
            resource_borrow_alias_idents: HashMap::new(),
            type_locations: HashMap::new(),
        };

        for (name, item) in import_items {
            generator.collect_world_item(name, item, true)?;
        }
        for (name, item) in export_items {
            generator.collect_world_item(name, item, false)?;
        }
        if generator.strip_interface_version {
            generator.validate_stripped_interface_name_collisions()?;
        }

        let mut named = generator
            .used_type_ids
            .iter()
            .copied()
            .filter(|id| {
                generator.resolve.types[*id].name.is_some() && !generator.is_adopted_type(*id)
            })
            .collect::<Vec<_>>();
        named.sort_by_key(|id| id.index());
        generator.named_type_ids = named;
        generator.assign_type_names();

        Ok(generator)
    }

    fn adopt_path_for(&self, wit_name: &str) -> Option<Path> {
        self.adopt_mappings
            .iter()
            .find(|mapping| wit_name.starts_with(&mapping.prefix.value()))
            .map(|mapping| mapping.path.clone())
    }

    fn is_adopted_interface(&self, id: InterfaceId) -> bool {
        self.resolve.interfaces[id]
            .package
            .map(|package| {
                let package = &self.resolve.packages[package];
                let interface_name = self.resolve.interfaces[id]
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("interface{}", id.index()));
                let wit_name = package.name.interface_id(&interface_name);
                self.adopt_path_for(&wit_name).is_some()
            })
            .unwrap_or(false)
    }

    fn is_adopted_type(&self, id: TypeId) -> bool {
        match self.resolve.types[id].owner {
            TypeOwner::Interface(interface_id) => self.is_adopted_interface(interface_id),
            _ => false,
        }
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
                    method_ident: function_method_ident(&function, &self.resolve),
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
        let adopt_path = self.adopt_path_for(wit_name);
        let location = if is_import {
            NamespaceLocation::Import(wit_name.to_owned())
        } else {
            NamespaceLocation::Export(wit_name.to_owned())
        };
        if adopt_path.is_none() {
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
        }

        let mut functions = Vec::new();
        for (name, function) in interface.functions {
            self.validate_function(&function)?;
            self.collect_function_types(&function)?;
            functions.push(FunctionBinding {
                wit_name: name.clone(),
                method_ident: function_method_ident(&function, &self.resolve),
                function,
            });
        }

        Ok(InterfaceNamespace {
            wit_name: wit_name.to_owned(),
            module_ident: interface_module_ident(wit_name, self.strip_interface_version),
            location,
            functions,
            adopt_path,
        })
    }

    fn validate_stripped_interface_name_collisions(&self) -> Result<()> {
        self.validate_interface_namespace_group("import", &self.import_interfaces)?;
        self.validate_interface_namespace_group("export", &self.export_interfaces)?;

        let direct_exports = self
            .direct_exports
            .iter()
            .map(|binding| (binding.method_ident.to_string(), binding.wit_name.as_str()))
            .collect::<HashMap<_, _>>();
        for namespace in &self.export_interfaces {
            let stripped = namespace.module_ident.to_string();
            if let Some(function) = direct_exports.get(&stripped) {
                return Err(Error::new(
                    Span::call_site(),
                    format!(
                        "stripping interface version produced export accessor `{stripped}` for `{}` that conflicts with direct export `{function}`",
                        namespace.wit_name
                    ),
                ));
            }
        }

        Ok(())
    }

    fn validate_interface_namespace_group(
        &self,
        kind: &str,
        namespaces: &[InterfaceNamespace],
    ) -> Result<()> {
        let mut seen = HashMap::<String, &str>::new();
        for namespace in namespaces {
            let stripped = namespace.module_ident.to_string();
            if let Some(previous) = seen.insert(stripped.clone(), namespace.wit_name.as_str()) {
                return Err(Error::new(
                    Span::call_site(),
                    format!(
                        "stripping interface version produced conflicting {kind} module `{stripped}` for `{previous}` and `{}`",
                        namespace.wit_name
                    ),
                ));
            }
        }
        Ok(())
    }

    fn assign_type_names(&mut self) {
        let mut named = self
            .used_type_ids
            .iter()
            .copied()
            .filter(|id| self.resolve.types[*id].name.is_some())
            .collect::<Vec<_>>();
        named.sort_by_key(|id| id.index());
        for type_id in named {
            let typedef = &self.resolve.types[type_id];
            let local = camel_ident(typedef.name.as_deref().unwrap());
            self.type_alias_idents.insert(type_id, local.clone());
            let unique = stable_type_ident(&self.resolve, self.world_id, type_id, false);
            self.type_unique_idents
                .insert(type_id, Ident::new(&unique, Span::call_site()));
            if matches!(typedef.kind, TypeDefKind::Resource) {
                let marker = stable_type_ident(&self.resolve, self.world_id, type_id, true);
                self.resource_marker_idents
                    .insert(type_id, Ident::new(&marker, Span::call_site()));
                let borrow_alias =
                    format!("{}Borrow", self.type_unique_idents.get(&type_id).unwrap());
                self.resource_borrow_alias_idents
                    .insert(type_id, Ident::new(&borrow_alias, Span::call_site()));
            }
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
        if !matches!(
            function.kind,
            FunctionKind::Freestanding
                | FunctionKind::Method(_)
                | FunctionKind::Static(_)
                | FunctionKind::Constructor(_)
        ) {
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
                    TypeDefKind::Handle(handle) => match handle {
                        Handle::Own(resource) | Handle::Borrow(resource) => {
                            self.collect_type_use(Type::Id(resource))?;
                        }
                    },
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
            const _: fn(&#runtime::Store) = |_store| {};
        })
    }

    fn generate_internal_support(&self) -> TokenStream2 {
        let runtime = &self.runtime_path;
        let bindgen = &self.bindgen_path;
        quote! {
            mod __internal {
                pub fn resolve_defined_type<'a>(
                    ty: &'a #bindgen::ValType,
                    program: &'a #runtime::ComponentProgram,
                ) -> Result<&'a #bindgen::Type, #runtime::ComponentError> {
                    match ty {
                        #bindgen::ValType::Type(type_id) => program
                            .get_type(*type_id)
                            .ok_or_else(|| #runtime::ComponentError::Link("type id not found".to_owned())),
                        #bindgen::ValType::Primitive(_) => Err(#runtime::ComponentError::Link(
                            "expected defined component type".to_owned(),
                        )),
                    }
                }

                pub fn lower_function_result<T>(
                    value: T,
                ) -> Result<Vec<#runtime::ComponentValue>, #runtime::ComponentError>
                where
                    T: #bindgen::ComponentReturn,
                {
                    <T as #bindgen::ComponentReturn>::into_component_results(value)
                }

                pub fn lift_function_result<T>(
                    results: Vec<#runtime::ComponentValue>,
                ) -> Result<T, #runtime::ComponentError>
                where
                    T: #bindgen::ComponentReturn,
                {
                    <T as #bindgen::ComponentReturn>::from_component_results(results)
                }

                pub fn missing_host(name: &str) -> #runtime::ComponentError {
                    #runtime::ComponentError::Trap(format!(
                        "host function `{name}` is not implemented"
                    ))
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
        let bindgen = &self.bindgen_path;
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
                            ty: &#bindgen::ValType,
                            program: &#runtime::ComponentProgram,
                        ) -> Result<(), #runtime::ComponentError> {
                            let #bindgen::Type::DefVal(#bindgen::DefValType::Record(fields)) =
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
                            ty: &#bindgen::ValType,
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
                            ty: &#bindgen::ValType,
                            program: &#runtime::ComponentProgram,
                        ) -> Result<(), #runtime::ComponentError> {
                            let #bindgen::Type::DefVal(#bindgen::DefValType::Variant(cases)) =
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
                            ty: &#bindgen::ValType,
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
                            ty: &#bindgen::ValType,
                            program: &#runtime::ComponentProgram,
                        ) -> Result<(), #runtime::ComponentError> {
                            let #bindgen::Type::DefVal(#bindgen::DefValType::Variant(cases)) =
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
                            ty: &#bindgen::ValType,
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
                            ty: &#bindgen::ValType,
                            program: &#runtime::ComponentProgram,
                        ) -> Result<(), #runtime::ComponentError> {
                            let #bindgen::Type::DefVal(#bindgen::DefValType::Flags(labels)) =
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
                            ty: &#bindgen::ValType,
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
                let ok = self.render_result_branch_internal_type(result.ok)?;
                let err = self.render_result_branch_internal_type(result.err)?;
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
            TypeDefKind::Resource => {
                let marker = self.resource_marker_idents.get(&type_id).unwrap();
                let borrow = self.resource_borrow_alias_idents.get(&type_id).unwrap();
                Ok(quote! {
                    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
                    pub struct #marker;
                    pub type #ident = #runtime::Own<#marker>;
                    pub type #borrow = #runtime::Borrow<#marker>;
                })
            }
            TypeDefKind::Handle(handle) => {
                let ty = self.render_named_handle_type(*handle, false)?;
                Ok(quote!(pub type #ident = #ty;))
            }
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
                    if let Some(prefix) = self.adopted_types_prefix(id) {
                        let ident = self.type_unique_idents.get(&id).unwrap();
                        return Ok(quote!(#prefix::types::#ident));
                    }
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
                        let ok = self.render_result_branch_internal_type(result.ok)?;
                        let err = self.render_result_branch_internal_type(result.err)?;
                        Ok(quote!(Result<#ok, #err>))
                    }
                    TypeDefKind::List(inner) | TypeDefKind::FixedLengthList(inner, _) => {
                        let ty = self.render_internal_type(*inner)?;
                        Ok(quote!(Vec<#ty>))
                    }
                    TypeDefKind::Type(inner) => self.render_internal_type(*inner),
                    TypeDefKind::Resource => {
                        let ident = self.type_unique_idents.get(&id).unwrap();
                        Ok(quote!(#ident))
                    }
                    TypeDefKind::Handle(handle) => self.render_named_handle_type(*handle, false),
                    TypeDefKind::Record(_)
                    | TypeDefKind::Variant(_)
                    | TypeDefKind::Enum(_)
                    | TypeDefKind::Flags(_) => Err(Error::new(
                        Span::call_site(),
                        "anonymous composite type is not supported by telomere-component-bindgen",
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
                    if let Some(prefix) = self.adopted_types_prefix(id) {
                        let ident = self.type_unique_idents.get(&id).unwrap();
                        return Ok(quote!(#prefix::types::#ident));
                    }
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
                        let ok = self.render_result_branch_public_type(result.ok, depth)?;
                        let err = self.render_result_branch_public_type(result.err, depth)?;
                        Ok(quote!(Result<#ok, #err>))
                    }
                    TypeDefKind::List(inner) | TypeDefKind::FixedLengthList(inner, _) => {
                        let ty = self.render_public_type(*inner, depth)?;
                        Ok(quote!(Vec<#ty>))
                    }
                    TypeDefKind::Type(inner) => self.render_public_type(*inner, depth),
                    TypeDefKind::Resource => {
                        let ident = self.type_unique_idents.get(&id).unwrap();
                        let prefix = match depth {
                            ScopeDepth::Root => quote!(types),
                            ScopeDepth::Nested => quote!(super::super::types),
                        };
                        Ok(quote!(#prefix::#ident))
                    }
                    TypeDefKind::Handle(handle) => {
                        self.render_unnamed_handle_public_type(*handle, depth)
                    }
                    TypeDefKind::Record(_)
                    | TypeDefKind::Variant(_)
                    | TypeDefKind::Enum(_)
                    | TypeDefKind::Flags(_) => Err(Error::new(
                        Span::call_site(),
                        "anonymous composite type is not supported by telomere-component-bindgen",
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
                    if let Some(borrow_unique) = self.resource_borrow_alias_idents.get(&type_id) {
                        let borrow_alias =
                            Ident::new(&format!("{}Borrow", alias), Span::call_site());
                        quote! {
                            pub use #prefix::#unique as #alias;
                            pub use #prefix::#borrow_unique as #borrow_alias;
                        }
                    } else {
                        quote!(pub use #prefix::#unique as #alias;)
                    }
                })
            })
            .collect()
    }

    fn render_named_handle_type(&self, handle: Handle, public: bool) -> Result<TokenStream2> {
        let runtime = &self.runtime_path;
        let (resource, borrowed) = match handle {
            Handle::Own(resource) => (resource, false),
            Handle::Borrow(resource) => (resource, true),
        };
        let resource = canonical_resource_type_id(&self.resolve, resource);
        if let Some(prefix) = self.adopted_types_prefix(resource) {
            let ident = if borrowed {
                self.resource_borrow_alias_idents
                    .get(&resource)
                    .ok_or_else(|| {
                        Error::new(Span::call_site(), "resource borrow alias is missing")
                    })?
            } else {
                self.type_unique_idents
                    .get(&resource)
                    .ok_or_else(|| Error::new(Span::call_site(), "resource alias is missing"))?
            };
            return Ok(quote!(#prefix::types::#ident));
        }
        let marker = self.resource_marker_idents.get(&resource).ok_or_else(|| {
            Error::new(
                Span::call_site(),
                format!(
                    "resource marker is missing for handle type `{}` (resource id {})",
                    self.resolve.types[resource]
                        .name
                        .as_deref()
                        .unwrap_or("<anonymous>"),
                    resource.index()
                ),
            )
        })?;
        if public {
            if borrowed {
                let borrow = self
                    .resource_borrow_alias_idents
                    .get(&resource)
                    .ok_or_else(|| {
                        Error::new(
                            Span::call_site(),
                            "resource borrow alias is missing for handle type",
                        )
                    })?;
                Ok(quote!(types::#borrow))
            } else {
                let ident = self.type_unique_idents.get(&resource).ok_or_else(|| {
                    Error::new(
                        Span::call_site(),
                        "resource alias is missing for handle type",
                    )
                })?;
                Ok(quote!(types::#ident))
            }
        } else if borrowed {
            Ok(quote!(#runtime::Borrow<#marker>))
        } else {
            Ok(quote!(#runtime::Own<#marker>))
        }
    }

    fn render_unnamed_handle_public_type(
        &self,
        handle: Handle,
        depth: ScopeDepth,
    ) -> Result<TokenStream2> {
        let (resource, borrowed) = match handle {
            Handle::Own(resource) => (resource, false),
            Handle::Borrow(resource) => (resource, true),
        };
        let resource = canonical_resource_type_id(&self.resolve, resource);
        if let Some(prefix) = self.adopted_types_prefix(resource) {
            let ident = if borrowed {
                self.resource_borrow_alias_idents
                    .get(&resource)
                    .ok_or_else(|| {
                        Error::new(Span::call_site(), "resource borrow alias is missing")
                    })?
            } else {
                self.type_unique_idents
                    .get(&resource)
                    .ok_or_else(|| Error::new(Span::call_site(), "resource alias is missing"))?
            };
            return Ok(quote!(#prefix::types::#ident));
        }

        let prefix = match depth {
            ScopeDepth::Root => quote!(types),
            ScopeDepth::Nested => quote!(super::super::types),
        };
        let ident = if borrowed {
            self.resource_borrow_alias_idents
                .get(&resource)
                .ok_or_else(|| Error::new(Span::call_site(), "resource borrow alias is missing"))?
        } else {
            self.type_unique_idents
                .get(&resource)
                .ok_or_else(|| Error::new(Span::call_site(), "resource alias is missing"))?
        };
        Ok(quote!(#prefix::#ident))
    }

    fn render_result_branch_internal_type(&self, ty: Option<Type>) -> Result<TokenStream2> {
        match ty {
            Some(ty) => self.render_internal_type(ty),
            None => Ok(quote!(())),
        }
    }

    fn render_result_branch_public_type(
        &self,
        ty: Option<Type>,
        depth: ScopeDepth,
    ) -> Result<TokenStream2> {
        match ty {
            Some(ty) => self.render_public_type(ty, depth),
            None => Ok(quote!(())),
        }
    }

    fn adopted_types_prefix(&self, type_id: TypeId) -> Option<Path> {
        let TypeOwner::Interface(interface_id) = self.resolve.types[type_id].owner else {
            return None;
        };
        let package = self.resolve.interfaces[interface_id].package?;
        let interface_name = self.resolve.interfaces[interface_id]
            .name
            .as_ref()
            .cloned()
            .unwrap_or_else(|| format!("interface{}", interface_id.index()));
        let wit_name = self.resolve.packages[package]
            .name
            .interface_id(&interface_name);
        self.adopt_path_for(&wit_name)
    }

    fn generate_root_imports(&self) -> Result<TokenStream2> {
        let runtime = &self.runtime_path;
        if self.direct_imports.is_empty() {
            return Ok(TokenStream2::new());
        }
        let sync_trait = if self.host_mode.includes_sync() {
            let methods = self
                .direct_imports
                .iter()
                .map(|binding| self.generate_import_trait_method(binding, ScopeDepth::Root, false))
                .collect::<Result<Vec<_>>>()?;
            Some(quote! {
                pub trait Imports {
                    #(#methods)*
                }
            })
        } else {
            None
        };
        let async_trait = if self.host_mode.includes_async() {
            let methods = self
                .direct_imports
                .iter()
                .map(|binding| self.generate_import_trait_method(binding, ScopeDepth::Root, true))
                .collect::<Result<Vec<_>>>()?;
            Some(quote! {
                pub trait ImportsAsync {
                    #(#methods)*
                }
            })
        } else {
            None
        };
        let sync_registrations = if self.host_mode.includes_sync() {
            Some(
                self.direct_imports
                    .iter()
                    .map(|binding| self.generate_root_import_registration(binding, false))
                    .collect::<Result<Vec<_>>>()?,
            )
        } else {
            None
        };
        let async_registrations = if self.host_mode.includes_async() {
            Some(
                self.direct_imports
                    .iter()
                    .map(|binding| self.generate_root_import_registration(binding, true))
                    .collect::<Result<Vec<_>>>()?,
            )
        } else {
            None
        };

        let sync_add = sync_registrations.map(|registrations| {
            quote! {
                pub fn add_root_imports_to_linker<T>(
                    linker: &mut #runtime::ComponentLinker,
                    host: ::std::rc::Rc<T>,
                )
                where
                    T: Imports + 'static,
                {
                    #(#registrations)*
                }
            }
        });
        let async_add = async_registrations.map(|registrations| {
            quote! {
                pub fn add_root_imports_to_linker_async<T>(
                    linker: &mut #runtime::ComponentLinker,
                    host: ::std::rc::Rc<T>,
                )
                where
                    T: ImportsAsync + 'static,
                {
                    #(#registrations)*
                }
            }
        });

        Ok(quote! {
            #sync_trait
            #async_trait
            #sync_add
            #async_add
        })
    }

    fn generate_import_interface(&self, namespace: &InterfaceNamespace) -> Result<TokenStream2> {
        let runtime = &self.runtime_path;
        let module_ident = &namespace.module_ident;
        if let Some(adopt_path) = &namespace.adopt_path {
            let import_path = quote!(#adopt_path::imports::#module_ident);
            return Ok(quote! {
                pub mod #module_ident {
                    pub use #import_path::*;
                }
            });
        }
        let aliases = self.generate_type_aliases(&namespace.location, ScopeDepth::Nested);
        let wit_name = LitStr::new(&namespace.wit_name, Span::call_site());
        let sync_trait = if self.host_mode.includes_sync() {
            let methods = namespace
                .functions
                .iter()
                .map(|binding| {
                    self.generate_import_trait_method(binding, ScopeDepth::Nested, false)
                })
                .collect::<Result<Vec<_>>>()?;
            Some(quote! {
                pub trait Host {
                    #(#methods)*
                }
            })
        } else {
            None
        };
        let async_trait = if self.host_mode.includes_async() {
            let methods = namespace
                .functions
                .iter()
                .map(|binding| self.generate_import_trait_method(binding, ScopeDepth::Nested, true))
                .collect::<Result<Vec<_>>>()?;
            Some(quote! {
                pub trait HostAsync {
                    #(#methods)*
                }
            })
        } else {
            None
        };
        let sync_add = if self.host_mode.includes_sync() {
            let registrations = namespace
                .functions
                .iter()
                .map(|binding| self.generate_instance_import_registration(binding, false))
                .collect::<Result<Vec<_>>>()?;
            Some(quote! {
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
            })
        } else {
            None
        };
        let async_add = if self.host_mode.includes_async() {
            let registrations = namespace
                .functions
                .iter()
                .map(|binding| self.generate_instance_import_registration(binding, true))
                .collect::<Result<Vec<_>>>()?;
            Some(quote! {
                pub fn add_to_linker_async<T>(
                    linker: &mut #runtime::ComponentLinker,
                    host: ::std::rc::Rc<T>,
                )
                where
                    T: HostAsync + 'static,
                {
                    let mut instance = #runtime::ComponentLinkerInstance::new();
                    #(#registrations)*
                    linker.register_import_instance(#wit_name, instance);
                }
            })
        } else {
            None
        };

        Ok(quote! {
            pub mod #module_ident {
                #(#aliases)*
                #sync_trait
                #async_trait
                #sync_add
                #async_add
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
        if let Some(adopt_path) = &namespace.adopt_path {
            let export_path = quote!(#adopt_path::exports::#module_ident::Exports);
            return Ok(quote! {
                pub mod #module_ident {
                    pub use #export_path as Exports;
                }
            });
        }
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
                    pub fn new(instance: #runtime::ComponentExports) -> Self {
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
        is_async: bool,
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
        let result = binding
            .function
            .result
            .map(|ty| self.render_public_type(ty, depth))
            .transpose()?
            .unwrap_or_else(|| quote!(()));
        let param_names = binding
            .function
            .params
            .iter()
            .map(|param| snake_ident(&param.name))
            .collect::<Vec<_>>();
        if is_async {
            Ok(quote! {
                fn #method_ident<'a>(
                    &'a self,
                    store: &'a #runtime::Store,
                    #(#params),*
                ) -> #runtime::ComponentFuture<'a, Result<#result, #runtime::ComponentError>> {
                    #(let _ = &#param_names;)*
                    Box::pin(async move {
                        let _ = store;
                        Err(#internal::missing_host(#wit_name))
                    })
                }
            })
        } else {
            Ok(quote! {
                fn #method_ident(
                    &self,
                    store: &#runtime::Store,
                    #(#params),*
                ) -> Result<#result, #runtime::ComponentError> {
                    let _ = store;
                    #(let _ = &#param_names;)*
                    Err(#internal::missing_host(#wit_name))
                }
            })
        }
    }

    fn generate_root_import_registration(
        &self,
        binding: &FunctionBinding,
        is_async: bool,
    ) -> Result<TokenStream2> {
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
        if is_async {
            Ok(quote! {
                linker.register_import_async(#wit_name, {
                    let host = ::std::rc::Rc::clone(&host);
                    move |store, args| {
                        let host = ::std::rc::Rc::clone(&host);
                        Box::pin(async move {
                            #arg_prep
                            let result: #result = host.#method_ident(store, #(#arg_names),*).await?;
                            __internal::lower_function_result::<#result>(result)
                        })
                    }
                });
            })
        } else {
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
    }

    fn generate_instance_import_registration(
        &self,
        binding: &FunctionBinding,
        is_async: bool,
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
        if is_async {
            Ok(quote! {
                instance.register_func_async(#wit_name, {
                    let host = ::std::rc::Rc::clone(&host);
                    move |store, args| {
                        let host = ::std::rc::Rc::clone(&host);
                        Box::pin(async move {
                            #arg_prep
                            let result: #result = host.#method_ident(store, #(#arg_names),*).await?;
                            super::super::__internal::lower_function_result::<#result>(result)
                        })
                    }
                });
            })
        } else {
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
                store: &#runtime::Store,
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

fn function_method_ident(function: &Function, resolve: &Resolve) -> Ident {
    let name = match function.kind {
        FunctionKind::Freestanding | FunctionKind::AsyncFreestanding => function.name.clone(),
        FunctionKind::Method(resource) | FunctionKind::AsyncMethod(resource) => format!(
            "{} {}",
            resolve.types[resource]
                .name
                .clone()
                .unwrap_or_else(|| format!("resource{}", resource.index())),
            function
                .name
                .rsplit('.')
                .next()
                .unwrap_or(function.name.as_str())
        ),
        FunctionKind::Static(resource) | FunctionKind::AsyncStatic(resource) => format!(
            "{} {}",
            resolve.types[resource]
                .name
                .clone()
                .unwrap_or_else(|| format!("resource{}", resource.index())),
            function
                .name
                .rsplit('.')
                .next()
                .unwrap_or(function.name.as_str())
        ),
        FunctionKind::Constructor(resource) => format!(
            "{} new",
            resolve.types[resource]
                .name
                .clone()
                .unwrap_or_else(|| format!("resource{}", resource.index()))
        ),
    };
    snake_ident(&name)
}

fn stable_type_ident(
    resolve: &Resolve,
    world_id: WorldId,
    type_id: TypeId,
    marker: bool,
) -> String {
    let typedef = &resolve.types[type_id];
    let mut parts = Vec::new();
    match typedef.owner {
        TypeOwner::World(id) => {
            parts.extend(package_prefix(resolve, resolve.worlds[id].package));
            parts.push(resolve.worlds[id].name.clone());
        }
        TypeOwner::Interface(id) => {
            parts.extend(package_prefix(resolve, resolve.interfaces[id].package));
            parts.push(
                resolve.interfaces[id]
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("interface{}", id.index())),
            );
        }
        TypeOwner::None => {
            parts.push(resolve.worlds[world_id].name.clone());
        }
    }
    parts.push(
        typedef
            .name
            .clone()
            .unwrap_or_else(|| format!("type{}", type_id.index())),
    );
    if marker {
        parts.push("marker".to_owned());
    }
    let mut candidate = parts.join(" ").to_upper_camel_case();
    if candidate.is_empty() {
        candidate = format!("GeneratedType{}", type_id.index());
    }
    if candidate.chars().next().unwrap().is_ascii_digit() {
        candidate.insert(0, '_');
    }
    if is_reserved(&candidate) {
        candidate.push('_');
    }
    candidate
}

fn canonical_resource_type_id(resolve: &Resolve, mut type_id: TypeId) -> TypeId {
    loop {
        match &resolve.types[type_id].kind {
            TypeDefKind::Resource => return type_id,
            TypeDefKind::Type(Type::Id(next)) => type_id = *next,
            _ => return type_id,
        }
    }
}

fn package_prefix(resolve: &Resolve, package: Option<PackageId>) -> Vec<String> {
    package
        .map(|package| {
            let package = &resolve.packages[package];
            vec![package.name.namespace.clone(), package.name.name.clone()]
        })
        .unwrap_or_default()
}

fn last_segment(text: &str) -> &str {
    text.rsplit('/')
        .next()
        .unwrap_or(text)
        .split('@')
        .next()
        .unwrap_or(text)
}

fn interface_module_ident(wit_name: &str, strip_interface_version: bool) -> Ident {
    let wit_name = if strip_interface_version {
        strip_interface_version_suffix(wit_name)
    } else {
        wit_name
    };
    snake_ident(wit_name)
}

fn strip_interface_version_suffix(wit_name: &str) -> &str {
    wit_name.split('@').next().unwrap_or(wit_name)
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
    let candidate = snake_name(text);
    Ident::new(&candidate, Span::call_site())
}

fn snake_name(text: &str) -> String {
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
    candidate
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

#[cfg(test)]
mod tests {
    use super::{expand, BindgenInput};
    use proc_macro2::{TokenStream as TokenStream2, TokenTree};
    use std::collections::BTreeSet;

    // Adding an item here extends the generated-code runtime contract.
    const ALLOWED_RUNTIME_ITEMS: &[&str] = &[
        "__bindgen",
        "Borrow",
        "ComponentError",
        "ComponentExports",
        "ComponentFuture",
        "ComponentInstance",
        "ComponentLinker",
        "ComponentLinkerInstance",
        "ComponentProgram",
        "ComponentValue",
        "LiftComponent",
        "LowerComponent",
        "Own",
        "Store",
    ];

    fn runtime_root_items(tokens: &TokenStream2) -> BTreeSet<String> {
        let mut items = BTreeSet::new();
        collect_runtime_root_items(tokens.clone(), &mut items);
        items
    }

    fn collect_runtime_root_items(tokens: TokenStream2, items: &mut BTreeSet<String>) {
        let tokens = tokens.into_iter().collect::<Vec<_>>();
        for token in &tokens {
            if let TokenTree::Group(group) = token {
                collect_runtime_root_items(group.stream(), items);
            }
        }

        for index in 0..tokens.len() {
            if let Some(item) = runtime_path_item(&tokens, index) {
                items.insert(item);
            }
        }
    }

    fn runtime_path_item(tokens: &[TokenTree], index: usize) -> Option<String> {
        if is_path_separator(tokens, index)
            && is_path_boundary(tokens, index)
            && path_ident(tokens, index + 2).as_deref() == Some("telomere_component")
            && is_path_separator(tokens, index + 3)
        {
            return path_ident(tokens, index + 5);
        }
        if path_ident(tokens, index).as_deref() == Some("crate")
            && is_path_separator(tokens, index + 1)
        {
            return path_ident(tokens, index + 3);
        }
        None
    }

    fn is_path_separator(tokens: &[TokenTree], index: usize) -> bool {
        matches!(
            (tokens.get(index), tokens.get(index + 1)),
            (Some(TokenTree::Punct(first)), Some(TokenTree::Punct(second)))
                if first.as_char() == ':' && second.as_char() == ':'
        )
    }

    fn is_path_boundary(tokens: &[TokenTree], index: usize) -> bool {
        match index
            .checked_sub(1)
            .and_then(|previous| tokens.get(previous))
        {
            Some(TokenTree::Ident(ident)) => {
                matches!(ident.to_string().as_str(), "as" | "impl" | "mut" | "use")
            }
            _ => true,
        }
    }

    fn path_ident(tokens: &[TokenTree], index: usize) -> Option<String> {
        match tokens.get(index) {
            Some(TokenTree::Ident(ident)) => Some(ident.to_string()),
            _ => None,
        }
    }

    #[test]
    fn strip_interface_version_is_opt_in() {
        let input: BindgenInput = syn::parse_str(
            r##"{
                inline: r#"
                    package ex:counterdemo@1.2.3;

                    interface service {
                        ping: func();
                    }

                    interface runner {
                        run: func();
                    }

                    world demo {
                        import service;
                        export runner;
                    }
                "#,
                world: "demo",
                module: "bindings"
            }"##,
        )
        .expect("bindgen input should parse");

        let tokens = expand(input).expect("bindgen should expand").to_string();
        assert!(tokens.contains("ex_counterdemo_service_1_2_3"));
        assert!(tokens.contains("ex_counterdemo_runner_1_2_3"));
    }

    #[test]
    fn strip_interface_version_removes_version_suffix_from_interface_modules() {
        let input: BindgenInput = syn::parse_str(
            r##"{
                inline: r#"
                    package ex:counterdemo@1.2.3;

                    interface service {
                        ping: func();
                    }

                    interface runner {
                        run: func();
                    }

                    world demo {
                        import service;
                        export runner;
                    }
                "#,
                world: "demo",
                module: "bindings",
                strip_interface_version: true
            }"##,
        )
        .expect("bindgen input should parse");

        let tokens = expand(input).expect("bindgen should expand").to_string();
        assert!(tokens.contains("pub mod ex_counterdemo_service"));
        assert!(!tokens.contains("ex_counterdemo_service_1_2_3"));
        assert!(tokens.contains("pub fn ex_counterdemo_runner"));
        assert!(!tokens.contains("ex_counterdemo_runner_1_2_3"));
    }

    #[test]
    fn strip_interface_version_rejects_export_accessor_collisions() {
        let input: BindgenInput = syn::parse_str(
            r##"{
                inline: r#"
                    package ex:counterdemo@1.2.3;

                    interface runner {
                        run: func();
                    }

                    world demo {
                        export runner;
                        export ex-counterdemo-runner: func();
                    }
                "#,
                world: "demo",
                strip_interface_version: true
            }"##,
        )
        .expect("bindgen input should parse");

        let error = expand(input).expect_err("colliding export names must fail");
        assert!(error
            .to_string()
            .contains("conflicts with direct export `ex-counterdemo-runner`"));
    }

    #[test]
    fn generated_module_keeps_missing_docs_allow() {
        let input: BindgenInput = syn::parse_str(
            r##"{
                inline: r#"
                    package ex:doclint;

                    world demo {
                        export ping: func();
                    }
                "#,
                world: "demo",
                module: "bindings"
            }"##,
        )
        .expect("bindgen input should parse");

        let tokens = expand(input).expect("bindgen should expand").to_string();
        assert!(
            tokens.contains("allow (missing_docs)"),
            "generated binding module lost its missing_docs allow: {tokens}"
        );
    }

    /// Expands representative inline WIT directly and recursively inspects its token paths.
    ///
    /// This intentionally avoids `cargo expand` and fixture compilation: the generator output
    /// itself defines the supported runtime surface contract.
    #[test]
    fn generated_code_uses_only_the_supported_runtime_surface() {
        let input: BindgenInput = syn::parse_str(
            r##"{
                inline: r#"
                    package ex:runtime-surface;

                    interface service {
                        resource counter {
                            constructor(seed: u32);
                            clone: static func(other: borrow<counter>) -> counter;
                            value: func() -> u32;
                        }
                    }

                    interface api {
                        ping: func() -> string;
                    }

                    world surface {
                        record payload {
                            value: u32,
                        }

                        variant issue {
                            missing,
                            malformed(string),
                        }

                        enum state {
                            ready,
                            done,
                        }

                        flags modes {
                            fast,
                            safe,
                        }

                        type maybe-payload = option<payload>;
                        type checked-payload = result<payload, issue>;
                        type payload-list = list<payload>;
                        type payload-pair = tuple<state, modes>;

                        import root-in: func(
                            payload: payload,
                            issue: issue,
                            state: state,
                            modes: modes,
                            maybe: maybe-payload,
                            checked: checked-payload,
                            values: payload-list,
                            pair: payload-pair,
                        ) -> checked-payload;
                        import telomere-component: func();
                        import service;

                        export root-out: func(
                            payload: payload,
                            maybe: maybe-payload,
                            checked: checked-payload,
                            values: payload-list,
                            pair: payload-pair,
                        ) -> payload;
                        export api;
                    }
                "#,
                world: "surface",
                module: "bindings",
                host_mode: "both"
            }"##,
        )
        .expect("bindgen input should parse");

        let tokens = expand(input).expect("bindgen should expand");
        let rendered = tokens.to_string();
        assert!(
            rendered.contains("fn telomere_component"),
            "the WIT identifier used to guard path-root detection was not emitted: {rendered}"
        );
        assert!(
            rendered.contains("__bindgen :: ComponentReturn"),
            "generated bindings must use the bindgen facade for ComponentReturn: {rendered}"
        );
        for forbidden in ["telomere_component :: ir :: types", "crate :: ir :: types"] {
            assert!(
                !rendered.contains(forbidden),
                "generated bindings must not expose the runtime IR path `{forbidden}`: {rendered}"
            );
        }

        let runtime_items = runtime_root_items(&tokens);
        let unexpected = runtime_items
            .iter()
            .filter(|item| !ALLOWED_RUNTIME_ITEMS.contains(&item.as_str()))
            .collect::<Vec<_>>();
        assert!(
            unexpected.is_empty(),
            "generated bindings referenced unsupported runtime root items {unexpected:?}; all items: {runtime_items:?}"
        );
        let expected_runtime_items = ALLOWED_RUNTIME_ITEMS
            .iter()
            .map(|item| (*item).to_owned())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            runtime_items, expected_runtime_items,
            "the representative WIT world must exercise every supported runtime root item"
        );
        assert!(
            !runtime_items.contains("ir") && !runtime_items.contains("ComponentReturn"),
            "internal runtime paths escaped the bindgen facade: {runtime_items:?}"
        );
    }
}
