mod component_decl;
mod core;
mod defval;
mod export_decl;
mod func;
mod generics_replace;
mod import_decl;
mod instance_decl;
mod primitive;
mod sort;
mod val;
use crate::decoder::{ComponentParseError, ParseResult, Validator};
use crate::ir::{ResourceId, TypeId};
pub use core::*;
pub use defval::*;
pub use export_decl::*;
pub use func::*;
pub use generics_replace::GenericsReplaceDSL;
pub use import_decl::*;
pub use primitive::*;
pub use sort::{CoreSortType, SortType};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
pub use val::*;

#[doc(hidden)]
#[derive(Clone, Debug)]
pub enum Type {
    DefVal(DefValType),
    Generic(Generic),
    Func(FuncType),
    Resource(ResourceId),
    Component(ComponentType),
    Instance(InstanceType),
}

impl Type {
    pub fn is_generic(&self) -> bool {
        matches!(self, Type::Generic(_))
    }
    pub fn is_resource(&self) -> bool {
        matches!(self, Type::Resource(_))
    }
    /// Whether this type denotes a resource in the broad sense a
    /// `(sub resource)` ascription accepts: a concrete resource, or an
    /// abstract resource face -- a `sub resource`-bounded generic such as
    /// the fresh identity minted by an earlier sub-ascribed export,
    /// possibly behind an `eq` alias.
    pub fn is_resource_or_face(&self, validator: &Validator) -> ParseResult<bool> {
        Ok(match self {
            Type::Resource(_) => true,
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            }) => true,
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => validator.get_type(*inner)?.is_resource_or_face(validator)?,
            _ => false,
        })
    }
    /// If this type resolves (through `Eq`-bound generics) to an
    /// abstract resource face -- a `Generic` with a `Sub` bound, as
    /// minted by `(sub resource)` imports and export ascriptions --
    /// returns that generic's nominal identity. Concrete resources and
    /// every other type resolve to `None`. The generic subtype
    /// relation accepts any two `Sub` generics without comparing
    /// identities, so equality assertions about faces must consult
    /// this identity explicitly.
    pub fn resource_face_id(&self, validator: &Validator) -> ParseResult<Option<usize>> {
        Ok(match self {
            Type::Generic(Generic {
                id,
                bound: GenericBound::Sub,
            }) => Some(*id),
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => validator.get_type(*inner)?.resource_face_id(validator)?,
            _ => None,
        })
    }
    /// Assert that this type and `other` -- the two sides of an
    /// `(eq ...)` ascription -- denote the same resource at every
    /// resource-reference leaf.
    ///
    /// The subtype relation alone cannot discharge an equality
    /// ascription: it accepts any two `Sub`-bounded generics (and any
    /// concrete resource against a `Sub` generic) without comparing
    /// identities, so references to two distinct abstract resources
    /// would otherwise be equated wherever they appear. This walk
    /// descends both types in parallel -- through defined value
    /// types, function types, and the entries of component/instance
    /// types -- and requires every pair of resource references it
    /// reaches to be the very same resource: the same concrete
    /// resource, or the same abstract identity (an imported abstract
    /// resource, or a `(sub resource)` face minted in this scope).
    /// Abstract types declared *inside* a component/instance type
    /// value are bound by that value rather than denoted by it, so
    /// pairs of them are matched by bound (alpha-equivalence), with a
    /// bijection keeping the sharing pattern consistent. A bound
    /// abstract identity can also be matched against an abstract
    /// identity denoted from the enclosing scopes, which instantiates
    /// it; the instantiation must be consistent (one bound identity,
    /// one instantiating identity) but need not be injective.
    /// Everything
    /// else about the two types remains the subtype check's business;
    /// this walk only adds the identity constraints.
    pub fn assert_same_resource_identities(
        &self,
        other: &Self,
        validator: &Validator,
    ) -> ParseResult<()> {
        let mut check = ResourceIdentityCheck {
            validator,
            bound_left: HashSet::new(),
            bound_right: HashSet::new(),
            alpha: HashMap::new(),
            alpha_rev: HashMap::new(),
            bound_assignment: HashMap::new(),
        };
        check.types(self, other)
    }
    pub fn is_component(&self) -> bool {
        matches!(self, Type::Component(_))
    }
    pub fn is_instance(&self) -> bool {
        matches!(self, Type::Instance(_))
    }
    pub fn is_func(&self) -> bool {
        matches!(self, Type::Func(_))
    }
    pub fn assert_subtype_of(&self, parent: &Self, validator: &Validator) -> ParseResult<()> {
        match (self, parent) {
            (Type::DefVal(a), Type::DefVal(b)) => {
                a.assert_subtype_of(b, validator)?;
                Ok(())
            }
            (
                Type::Generic(Generic {
                    id: _,
                    bound: GenericBound::Eq(a),
                }),
                b,
            ) => validator.get_type(*a)?.assert_subtype_of(b, validator),
            (
                Type::Generic(Generic {
                    id: _,
                    bound: GenericBound::Sub,
                }),
                Type::Generic(Generic {
                    id: _,
                    bound: parent_bound,
                }),
            ) => GenericBound::Sub.assert_subtype_of(parent_bound, validator),
            (
                Type::Generic(Generic {
                    id: _,
                    bound: GenericBound::Sub,
                }),
                _,
            ) => Err(ComponentParseError::TypeMismatch(
                "generic resource cannot assign to concrete type".to_owned(),
            )),
            (a, Type::Generic(parent)) => match &parent.bound {
                GenericBound::Eq(b) => a.assert_subtype_of(validator.get_type(*b)?, validator),
                GenericBound::Sub => {
                    if a.is_resource() {
                        Ok(())
                    } else {
                        Err(ComponentParseError::TypeMismatch(
                            "expected any resource".to_owned(),
                        ))
                    }
                }
            },
            (Type::Func(a), Type::Func(b)) => a.assert_subtype_of(b, validator),
            (Type::Resource(a), Type::Resource(b)) => {
                if a == b {
                    Ok(())
                } else {
                    Err(ComponentParseError::TypeMismatch(
                        "resource types are not the same".to_owned(),
                    ))
                }
            }
            (Type::Component(a), Type::Component(b)) => a.assert_subtype_of(b, validator),
            (Type::Instance(a), Type::Instance(b)) => a.assert_subtype_of(b, validator),
            (a, b) => {
                tracing::trace!("resource kind mismatch {a:?} {b:?}");
                Err(ComponentParseError::TypeMismatch(
                    "resource kind mismatch".to_owned(),
                ))
            }
        }
    }
}

/// Parallel identity walk behind
/// [`Type::assert_same_resource_identities`]. `bound_left` /
/// `bound_right` hold the generic identities declared inside the
/// component/instance type values currently being descended into
/// (per side), and `alpha` / `alpha_rev` record how those bound
/// generics have been paired within the comparisons that currently
/// bind them (a pairing is dropped when a binder it mentions leaves
/// scope, alongside the bound sets).
struct ResourceIdentityCheck<'v> {
    validator: &'v Validator<'v>,
    bound_left: HashSet<usize>,
    bound_right: HashSet<usize>,
    alpha: HashMap<usize, usize>,
    alpha_rev: HashMap<usize, usize>,
    /// Instantiations of bound abstract identities by abstract
    /// identities denoted from the enclosing scopes, keyed by the
    /// bound generic's id (on whichever side declares it) and valued
    /// by the instantiating generic's id. Like the alpha pairings, an
    /// entry is dropped when its binder leaves scope. An entry also
    /// constrains the binder's alpha partner: paired binders denote
    /// one identity, so an instantiation recorded for either of them
    /// must agree with any instantiation recorded for the other.
    bound_assignment: HashMap<usize, usize>,
}

impl<'v> ResourceIdentityCheck<'v> {
    fn mismatch<T>() -> ParseResult<T> {
        Err(ComponentParseError::TypeMismatch(
            "resource types are not the same".to_owned(),
        ))
    }

    fn types(&mut self, a: &Type, b: &Type) -> ParseResult<()> {
        // Resolve `Eq`-bound generics to the types they alias; what
        // remains is a concrete type, a structural type, or a
        // `Sub`-bounded generic (an abstract resource identity).
        let mut a = a;
        while let Type::Generic(Generic {
            bound: GenericBound::Eq(inner),
            ..
        }) = a
        {
            a = self.validator.get_type(*inner)?;
        }
        let mut b = b;
        while let Type::Generic(Generic {
            bound: GenericBound::Eq(inner),
            ..
        }) = b
        {
            b = self.validator.get_type(*inner)?;
        }
        match (a, b) {
            (Type::Generic(ga), Type::Generic(gb)) => self.generic_pair(ga, gb),
            (Type::Generic(_), Type::Resource(_)) | (Type::Resource(_), Type::Generic(_)) => {
                Self::mismatch()
            }
            (Type::Resource(x), Type::Resource(y)) => {
                if x == y {
                    Ok(())
                } else {
                    Self::mismatch()
                }
            }
            (Type::DefVal(x), Type::DefVal(y)) => self.defvals(x, y),
            (Type::Func(x), Type::Func(y)) => self.funcs(x, y),
            (Type::Component(x), Type::Component(y)) => self.components(x, y),
            (Type::Instance(x), Type::Instance(y)) => self.instances(x, y),
            // Every other pairing is a kind mismatch or a structural
            // question the subtype check already decides; resource
            // identity has nothing to add.
            _ => Ok(()),
        }
    }

    /// Judge a pair of `Sub`-bounded generics: the same declaration
    /// always matches; two generics bound by the type values being
    /// descended into match by alpha-equivalence (recorded in the
    /// bijection, so a sharing pattern must be consistent); a bound
    /// generic matched against a free one is instantiated by it
    /// (recorded in `bound_assignment`, so the instantiation must be
    /// consistent -- including across an alpha pairing, since paired
    /// binders denote one identity); anything else equates two
    /// distinct abstract identities.
    fn generic_pair(&mut self, a: &Generic, b: &Generic) -> ParseResult<()> {
        if a.id == b.id {
            return Ok(());
        }
        let a_bound = self.bound_left.contains(&a.id);
        let b_bound = self.bound_right.contains(&b.id);
        match (a_bound, b_bound) {
            (true, true) => match self.alpha.get(&a.id) {
                Some(&paired) if paired == b.id => Ok(()),
                Some(_) => Self::mismatch(),
                None => {
                    if self.alpha_rev.contains_key(&b.id) {
                        return Self::mismatch();
                    }
                    // The pairing makes the two binders denote one
                    // identity, so instantiations already recorded
                    // for them (by earlier bound-to-free matches) must
                    // agree; checking here as well as in `assign_bound`
                    // keeps the verdict independent of traversal order.
                    if let (Some(&fa), Some(&fb)) = (
                        self.bound_assignment.get(&a.id),
                        self.bound_assignment.get(&b.id),
                    ) {
                        if fa != fb {
                            return Self::mismatch();
                        }
                    }
                    self.alpha.insert(a.id, b.id);
                    self.alpha_rev.insert(b.id, a.id);
                    Ok(())
                }
            },
            (true, false) => self.assign_bound(a.id, b.id),
            (false, true) => self.assign_bound(b.id, a.id),
            (false, false) => Self::mismatch(),
        }
    }

    /// Record that the bound abstract identity `bound` is
    /// instantiated by the free identity `free`. The same bound
    /// identity must be instantiated the same way everywhere it is
    /// matched within the scope that binds it; distinct bound
    /// identities may share one instantiating identity. If `bound`
    /// is alpha-paired with a binder on the other side, the pairing
    /// makes the two denote one identity, so an instantiation
    /// recorded for the partner constrains this one (and vice versa)
    /// -- otherwise two matches could independently assign the
    /// paired binders to distinct free identities, equating the
    /// latter through the pairing.
    fn assign_bound(&mut self, bound: usize, free: usize) -> ParseResult<()> {
        let partner = self
            .alpha
            .get(&bound)
            .copied()
            .or_else(|| self.alpha_rev.get(&bound).copied());
        for key in [Some(bound), partner].into_iter().flatten() {
            match self.bound_assignment.get(&key) {
                Some(&prev) if prev == free => {}
                Some(_) => return Self::mismatch(),
                None => {}
            }
        }
        self.bound_assignment.insert(bound, free);
        Ok(())
    }

    fn ids(&mut self, a: TypeId, b: TypeId) -> ParseResult<()> {
        if a == b {
            return Ok(());
        }
        let validator = self.validator;
        let a_ty = validator.get_type(a)?;
        let b_ty = validator.get_type(b)?;
        self.types(a_ty, b_ty)
    }

    fn valtypes(&mut self, a: &ValType, b: &ValType) -> ParseResult<()> {
        if let (ValType::Type(x), ValType::Type(y)) = (a, b) {
            self.ids(*x, *y)?;
        }
        Ok(())
    }

    fn defvals(&mut self, a: &DefValType, b: &DefValType) -> ParseResult<()> {
        use DefValType::*;
        match (a, b) {
            (Record(fields), Record(parent_fields)) => {
                for (field, parent_field) in fields.iter().zip(parent_fields.iter()) {
                    self.valtypes(&field.ty, &parent_field.ty)?;
                }
            }
            (Variant(cases), Variant(parent_cases)) => {
                for (case, parent_case) in cases.iter().zip(parent_cases.iter()) {
                    if let (Some(ty), Some(parent_ty)) = (&case.ty, &parent_case.ty) {
                        self.valtypes(ty, parent_ty)?;
                    }
                }
            }
            (List(ty, _), List(parent_ty, _)) => self.valtypes(ty, parent_ty)?,
            (Own(x), Own(y)) | (Borrow(x), Borrow(y)) => self.ids(*x, *y)?,
            // Primitives and flags carry no resource references, and
            // mismatched shapes are the subtype check's business.
            _ => {}
        }
        Ok(())
    }

    fn funcs(&mut self, a: &FuncType, b: &FuncType) -> ParseResult<()> {
        for (param, parent_param) in a.params.iter().zip(b.params.iter()) {
            self.valtypes(param, parent_param)?;
        }
        if let (Some(result), Some(parent_result)) = (&a.result, &b.result) {
            self.valtypes(result, parent_result)?;
        }
        Ok(())
    }

    fn components(&mut self, a: &ComponentType, b: &ComponentType) -> ParseResult<()> {
        let restore = self.extend_bounds(
            &bound_generics_of_component(self.validator, a),
            &bound_generics_of_component(self.validator, b),
        );
        let result = self.component_entries(a, b);
        self.restore_bounds(restore);
        result
    }

    fn component_entries(&mut self, a: &ComponentType, b: &ComponentType) -> ParseResult<()> {
        for (name, import) in &a.imports {
            let Some(parent_import) = b.imports.get(name) else {
                continue;
            };
            match (import, parent_import) {
                (
                    ComponentImportType::Type { generic: ga, .. },
                    ComponentImportType::Type { generic: gb, .. },
                ) => match (&ga.bound, &gb.bound) {
                    (GenericBound::Sub, GenericBound::Sub) => self.generic_pair(ga, gb)?,
                    (GenericBound::Eq(x), GenericBound::Eq(y)) => self.ids(*x, *y)?,
                    _ => {}
                },
                (ComponentImportType::Value(x), ComponentImportType::Value(y)) => {
                    self.valtypes(x, y)?;
                }
                _ => {}
            }
        }
        for (name, export) in &a.exports {
            let Some(parent_export) = b.exports.get(name) else {
                continue;
            };
            use ComponentExportType::*;
            match (export, parent_export) {
                (Type(x), Type(y))
                | (Component(x), Component(y))
                | (Instance(x), Instance(y))
                | (Func(x), Func(y)) => self.ids(*x, *y)?,
                (Value(x), Value(y)) => self.valtypes(x, y)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn instances(&mut self, a: &InstanceType, b: &InstanceType) -> ParseResult<()> {
        let restore = self.extend_bounds(
            &bound_generics_of_instance(self.validator, a),
            &bound_generics_of_instance(self.validator, b),
        );
        let result = self.instance_entries(a, b);
        self.restore_bounds(restore);
        result
    }

    fn instance_entries(&mut self, a: &InstanceType, b: &InstanceType) -> ParseResult<()> {
        for (name, export) in &a.exports {
            let Some(parent_export) = b.exports.get(name) else {
                continue;
            };
            use InstanceExportType::*;
            match (export, parent_export) {
                (Type(x), Type(y))
                | (Component(x), Component(y))
                | (Instance(x), Instance(y))
                | (Func(x), Func(y)) => self.ids(*x, *y)?,
                (Value(x), Value(y)) => self.valtypes(x, y)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn extend_bounds(
        &mut self,
        left: &HashSet<usize>,
        right: &HashSet<usize>,
    ) -> (Vec<usize>, Vec<usize>) {
        let added_left: Vec<usize> = left
            .iter()
            .copied()
            .filter(|id| self.bound_left.insert(*id))
            .collect();
        let added_right: Vec<usize> = right
            .iter()
            .copied()
            .filter(|id| self.bound_right.insert(*id))
            .collect();
        (added_left, added_right)
    }

    fn restore_bounds(&mut self, (left, right): (Vec<usize>, Vec<usize>)) {
        for id in &left {
            self.bound_left.remove(id);
        }
        for id in &right {
            self.bound_right.remove(id);
        }
        // Alpha pairings belong to the binders that established them:
        // a pairing mentioning a binder whose scope is being exited
        // must not leak into later, independent comparisons, where
        // the same binder may legitimately pair with a different
        // counterpart. Pairings between binders that remain in scope
        // persist, so a sharing pattern spanning nested values stays
        // consistent for the rest of the enclosing comparison.
        if !left.is_empty() || !right.is_empty() {
            self.alpha
                .retain(|l, r| !left.contains(l) && !right.contains(r));
            self.alpha_rev
                .retain(|r, l| !right.contains(r) && !left.contains(l));
            self.bound_assignment
                .retain(|bound, _| !left.contains(bound) && !right.contains(bound));
        }
    }
}

/// The generic identities a component type declares in its own
/// scope: its type imports, plus every abstract identity reachable
/// from its entries through value types defined inside the value
/// (references to types visible in the enclosing scopes are free
/// references, not declarations of this value, and are not followed;
/// nested component and instance types are scope boundaries whose
/// own declarations are collected when the walk descends into them).
fn bound_generics_of_component(validator: &Validator, ty: &ComponentType) -> HashSet<usize> {
    let mut out = HashSet::new();
    let mut seen = HashSet::new();
    for import in ty.imports.values() {
        match import {
            ComponentImportType::Type { generic, .. } => match &generic.bound {
                GenericBound::Sub => {
                    out.insert(generic.id);
                }
                GenericBound::Eq(inner) => collect_bound_id(validator, *inner, &mut out, &mut seen),
            },
            ComponentImportType::Value(val) => {
                collect_bound_valtype(validator, val, &mut out, &mut seen)
            }
            ComponentImportType::CoreModule(_) => {}
        }
    }
    for export in ty.exports.values() {
        use ComponentExportType::*;
        match export {
            Type(id) | Component(id) | Instance(id) | Func(id) => {
                collect_bound_id(validator, *id, &mut out, &mut seen)
            }
            Value(val) => collect_bound_valtype(validator, val, &mut out, &mut seen),
            CoreModule(_) => {}
        }
    }
    out
}

/// The generic identities an instance type declares for itself,
/// collected the same way as for a component type.
fn bound_generics_of_instance(validator: &Validator, ty: &InstanceType) -> HashSet<usize> {
    let mut out = HashSet::new();
    let mut seen = HashSet::new();
    for export in ty.exports.values() {
        use InstanceExportType::*;
        match export {
            Type(id) | Component(id) | Instance(id) | Func(id) => {
                collect_bound_id(validator, *id, &mut out, &mut seen)
            }
            Value(val) => collect_bound_valtype(validator, val, &mut out, &mut seen),
            CoreModule(_) => {}
        }
    }
    out
}

fn collect_bound_id(
    validator: &Validator,
    id: TypeId,
    out: &mut HashSet<usize>,
    seen: &mut HashSet<TypeId>,
) {
    // A type visible from the enclosing scopes is a free reference:
    // whatever it denotes is judged where the walk reaches it, and
    // its own generics are not declarations of the value being
    // collected from.
    if validator.type_id_visible_in_scope_stack(id) || !seen.insert(id) {
        return;
    }
    let Ok(ty) = validator.get_type(id) else {
        return;
    };
    match ty {
        Type::Generic(generic) => match &generic.bound {
            GenericBound::Sub => {
                out.insert(generic.id);
            }
            GenericBound::Eq(inner) => collect_bound_id(validator, *inner, out, seen),
        },
        Type::DefVal(def) => collect_bound_defval(validator, def, out, seen),
        Type::Func(func) => {
            for param in &func.params {
                collect_bound_valtype(validator, param, out, seen);
            }
            if let Some(result) = &func.result {
                collect_bound_valtype(validator, result, out, seen);
            }
        }
        // A nested component or instance type is a scope boundary:
        // the generics it declares belong to that nested comparison,
        // which collects them itself when the walk descends into the
        // pair (see `components` / `instances`). Hoisting them into
        // the enclosing bound set would leave nothing for the nested
        // `extend_bounds` to add, so the nested scope's alpha pairings
        // could not be restored on exit and would leak into sibling
        // entries.
        Type::Component(_) | Type::Instance(_) | Type::Resource(_) => {}
    }
}

fn collect_bound_valtype(
    validator: &Validator,
    val: &ValType,
    out: &mut HashSet<usize>,
    seen: &mut HashSet<TypeId>,
) {
    if let ValType::Type(id) = val {
        collect_bound_id(validator, *id, out, seen);
    }
}

fn collect_bound_defval(
    validator: &Validator,
    def: &DefValType,
    out: &mut HashSet<usize>,
    seen: &mut HashSet<TypeId>,
) {
    use DefValType::*;
    match def {
        Record(fields) => {
            for field in fields {
                collect_bound_valtype(validator, &field.ty, out, seen);
            }
        }
        Variant(cases) => {
            for case in cases.iter() {
                if let Some(ty) = &case.ty {
                    collect_bound_valtype(validator, ty, out, seen);
                }
            }
        }
        List(ty, _) => collect_bound_valtype(validator, ty, out, seen),
        Own(id) | Borrow(id) => collect_bound_id(validator, *id, out, seen),
        Primitive(_) | Flags(_) => {}
    }
}

#[derive(Clone, Debug)]
pub struct Generic {
    pub id: usize,
    pub bound: GenericBound,
}

impl Generic {
    pub fn new(bound: GenericBound) -> Self {
        static GENERIC_ID: AtomicUsize = AtomicUsize::new(0);
        Self {
            id: GENERIC_ID.fetch_add(1, Ordering::Relaxed),
            bound,
        }
    }
}

#[derive(Clone, Debug)]
pub enum GenericBound {
    Eq(TypeId),
    Sub,
}

#[derive(Clone, Debug)]
pub enum ComponentImportType {
    Type { type_id: TypeId, generic: Generic },
    CoreModule(CoreModuleType),
    Value(ValType),
}
impl GenericBound {
    pub fn assert_subtype_of(
        &self,
        parent: &GenericBound,
        validator: &Validator,
    ) -> ParseResult<()> {
        match (self, parent) {
            (GenericBound::Eq(a), GenericBound::Eq(b)) => a.assert_subtype_of(*b, validator)?,
            (GenericBound::Eq(type_id), GenericBound::Sub) => {
                if !validator.get_type(*type_id)?.is_resource() {
                    Err(ComponentParseError::TypeMismatch(
                        "expected any resource".to_owned(),
                    ))?
                }
            }
            (GenericBound::Sub, GenericBound::Eq(_type_id)) => {
                // FIMXE: sould retrive type_id and validate it?
                Err(ComponentParseError::TypeMismatch(
                    "sub resource cannot assign to except sub resource".to_owned(),
                ))?
            }
            (GenericBound::Sub, GenericBound::Sub) => {
                // ok
            }
        };
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct ComponentType {
    pub import_order: Vec<String>,
    pub imports: HashMap<String, ComponentImportType>,
    pub exports: HashMap<String, ComponentExportType>,
    pub generics_replacing_program: Vec<GenericsReplaceDSL>,
}
impl ComponentType {
    pub fn assert_subtype_of(
        &self,
        parent: &ComponentType,
        validator: &Validator,
    ) -> ParseResult<()> {
        if self.imports.len() > parent.imports.len() {
            Err(ComponentParseError::TypeMismatch(
                "import count mismatch".to_owned(),
            ))?
        }
        for (child_entry_name, child_ty) in &self.imports {
            let parent_ty = parent.imports.get(child_entry_name).ok_or_else(|| {
                ComponentParseError::TypeMismatch("import name mismatch".to_owned())
            })?;
            match (child_ty, parent_ty) {
                (
                    ComponentImportType::Type {
                        generic: child,
                        type_id: _,
                    },
                    ComponentImportType::Type {
                        generic: parent,
                        type_id: _,
                    },
                ) => child.bound.assert_subtype_of(&parent.bound, validator)?,
                (
                    ComponentImportType::CoreModule(child),
                    ComponentImportType::CoreModule(parent),
                ) if child == parent => {}
                (ComponentImportType::Value(child), ComponentImportType::Value(parent)) => {
                    child.assert_subtype_of(parent, validator)?
                }
                _ => Err(ComponentParseError::TypeMismatch(
                    "import kind mismatch".to_owned(),
                ))?,
            }
        }
        if parent.exports.len() > self.exports.len() {
            Err(ComponentParseError::TypeMismatch(
                "export count mismatch".to_owned(),
            ))?
        }
        for (parent_entry_name, expected_ty) in &parent.exports {
            let actual_ty = self.exports.get(parent_entry_name).ok_or_else(|| {
                ComponentParseError::TypeMismatch("import name mismatch".to_owned())
            })?;
            expected_ty.assert_subtype_of(actual_ty, validator)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub enum ComponentExportType {
    CoreModule(CoreModuleType),
    Component(TypeId),
    Instance(TypeId),
    Type(TypeId),
    Func(TypeId),
    Value(ValType),
    //Resource(ResourceId),
    //NewResource(TypeId),
}

impl ComponentExportType {
    pub fn assert_subtype_of<'a>(
        &self,
        parent: &Self,
        validator: &'a Validator<'a>,
    ) -> ParseResult<()> {
        use ComponentExportType::*;
        match (self, parent) {
            (CoreModule(a), CoreModule(b)) => {
                if a != b {
                    Err(ComponentParseError::TypeMismatch(
                        "core module export mismatch".to_owned(),
                    ))?
                }
            }
            (Component(a), Component(b)) => {
                validator.assert_component_type_ids_subtype_of(*a, *b)?;
            }
            (Instance(a), Instance(b)) => {
                validator.assert_instance_type_ids_subtype_of(*a, *b)?;
            }
            (Type(a), Type(b)) => {
                validator.assert_type_ids_subtype_of(*a, *b)?;
            }
            (Func(a), Func(b)) => validator
                .get_func_type(*a)?
                .assert_subtype_of(validator.get_func_type(*b)?, validator)?,
            (Value(a), Value(b)) => a.assert_subtype_of(b, validator)?,
            _ => Err(ComponentParseError::TypeMismatch(
                "export kind mismatch".to_owned(),
            ))?,
        };
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct InstanceType {
    pub exports: HashMap<String, InstanceExportType>,
}

impl InstanceType {
    pub fn get_export(&self, name: &String) -> ParseResult<&InstanceExportType> {
        self.exports
            .get(name)
            .ok_or_else(|| ComponentParseError::ExportNotFound(name.clone()))
    }
    pub fn assert_subtype_of(&self, parent: &Self, validator: &Validator) -> ParseResult<()> {
        if self.exports.len() < parent.exports.len() {
            Err(ComponentParseError::TypeMismatch(
                "instance export count".to_owned(),
            ))?
        }
        for (name, ty) in &parent.exports {
            self.get_export(name)?.assert_subtype_of(ty, validator)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum InstanceExportType {
    CoreModule(CoreModuleType),
    Func(TypeId),
    Component(TypeId),
    Instance(TypeId),
    //Resource(ResourceId),
    Type(TypeId),
    //SubResource(TypeId),
    Value(ValType),
}
impl InstanceExportType {
    pub fn assert_subtype_of(&self, parent: &Self, validator: &Validator) -> ParseResult<()> {
        use InstanceExportType::*;
        match (self, parent) {
            (CoreModule(a), CoreModule(b)) => {
                if a != b {
                    Err(ComponentParseError::TypeMismatch(
                        "core module export mismatch".to_owned(),
                    ))?
                }
                Ok(())
            }
            (Func(a), Func(b)) => validator
                .get_func_type(*a)?
                .assert_subtype_of(validator.get_func_type(*b)?, validator),
            (Component(a), Component(b)) => validator.assert_component_type_ids_subtype_of(*a, *b),
            (Instance(a), Instance(b)) => validator.assert_instance_type_ids_subtype_of(*a, *b),
            (Type(a), Type(b)) => validator.assert_type_ids_subtype_of(*a, *b),
            (Value(a), Value(b)) => a.assert_subtype_of(b, validator),
            _ => Err(ComponentParseError::TypeMismatch(
                "export kind mismatch".to_owned(),
            ))?,
        }
    }
}
