use crate::decoder::{ParseResult, TransformContext, Validator};
use crate::ir::types::{CoreModuleType, Generic, GenericBound, Type, ValType};
use crate::ir::TypeId;
use std::collections::HashMap;

use super::InstanceExportType;

// These instructions model component exports, so their names retain the
// `Export` prefix used by the decoder transform.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone)]
pub enum GenericsReplaceDSL {
    ExportCoreModule(String, CoreModuleType),
    ExportComponent(String, TypeId),
    ExportInstance(String, TypeId),
    ExportFunc(String, TypeId),
    ExportTypeEq(String, TypeId),
    ExportTypeSub(String, TypeId),
    ExportValue(String, ValType),
}
pub struct GenericsReplaceDSLEnvironment {
    result: HashMap<String, InstanceExportType>,
    unified: TransformContext,
}
fn resolve_generics(
    ty: TypeId,
    validator: &mut Validator,
    unified: &mut TransformContext,
) -> ParseResult<TypeId> {
    validator.instantiate_type_id(ty, unified)
}

/// Resolve the target of an `ExportTypeEq` instruction. When the
/// exported type is itself a resource tied to a `(sub resource)`
/// export face, the plain export keeps its own instantiation of the
/// underlying resource -- distinct from the face's -- while every
/// other reference in the program follows the tie. The untied
/// instantiation is computed with the tie suspended and cached in
/// the context's plain-root table, apart from the ordinary mapping,
/// so the two instantiations never pre-empt each other regardless
/// of the order the exports are declared in.
fn resolve_export_type_eq(
    ty: TypeId,
    validator: &mut Validator,
    unified: &mut TransformContext,
) -> ParseResult<TypeId> {
    // Resolve eq aliases to the type the export actually denotes.
    let mut root = ty;
    while let Ok(Type::Generic(Generic {
        bound: GenericBound::Eq(inner),
        ..
    })) = validator.get_type(root)
    {
        root = *inner;
    }
    let tied_face = match unified.tie_target(root) {
        // Only the resource itself is exempt; an export of one of
        // its faces, or of any other type, resolves through the tie
        // like any other reference.
        Some(face) if validator.sub_face_root(root).is_none() => Some(face),
        _ => None,
    };
    let Some(face) = tied_face else {
        return resolve_generics(ty, validator, unified);
    };
    if let Some(mapped) = unified.plain_root(root) {
        return Ok(mapped);
    }
    unified.remove_tie(root);
    let resolved = resolve_generics(ty, validator, unified);
    unified.restore_tie(root, face);
    let mapped = resolved?;
    // The untied resolution recorded ordinary mappings for the
    // exported type and its root; drop them so later references keep
    // following the tie, and remember the result as the plain root.
    unified.remove(ty);
    unified.remove(root);
    unified.insert_plain_root(root, mapped);
    Ok(mapped)
}
impl GenericsReplaceDSL {
    pub fn evaluate_one(
        instr: &GenericsReplaceDSL,
        validator: &mut Validator,
        env: &mut GenericsReplaceDSLEnvironment,
    ) -> ParseResult<()> {
        let GenericsReplaceDSLEnvironment { result, unified } = env;

        match instr {
            GenericsReplaceDSL::ExportCoreModule(name, ty) => {
                result.insert(name.clone(), InstanceExportType::CoreModule(ty.clone()));
            }
            GenericsReplaceDSL::ExportComponent(name, type_id) => {
                result.insert(
                    name.clone(),
                    InstanceExportType::Component(
                        validator.resolve_surface_type_id(*type_id, unified)?,
                    ),
                );
            }
            GenericsReplaceDSL::ExportInstance(name, type_id) => {
                result.insert(
                    name.clone(),
                    InstanceExportType::Instance(
                        validator.resolve_surface_type_id(*type_id, unified)?,
                    ),
                );
            }
            GenericsReplaceDSL::ExportFunc(name, type_id) => {
                result.insert(
                    name.clone(),
                    InstanceExportType::Func(resolve_generics(*type_id, validator, unified)?),
                );
            }
            GenericsReplaceDSL::ExportTypeEq(name, type_id) => {
                result.insert(
                    name.clone(),
                    InstanceExportType::Type(resolve_export_type_eq(*type_id, validator, unified)?),
                );
            }
            GenericsReplaceDSL::ExportTypeSub(name, type_id) => {
                let new_type_id = validator.instantiate_sub_resource_type(*type_id, unified)?;
                result.insert(name.clone(), InstanceExportType::Type(new_type_id));
            }
            GenericsReplaceDSL::ExportValue(name, ty) => {
                let ty = validator.instantiate_valtype(ty, unified)?;
                result.insert(name.clone(), InstanceExportType::Value(ty));
            }
        }
        Ok(())
    }
    pub(crate) fn evaluate(
        program: &[GenericsReplaceDSL],
        validator: &mut Validator,
        initial_unified: TransformContext,
    ) -> ParseResult<HashMap<String, InstanceExportType>> {
        let mut env = GenericsReplaceDSLEnvironment {
            result: HashMap::new(),
            unified: initial_unified,
        };
        // Roots the program also exports plainly -- an
        // `ExportTypeEq` of the resource itself, as opposed to one
        // of its faces. A plain export keeps the resource's own
        // instantiation (see `resolve_export_type_eq`), and
        // references to the resource elsewhere in the program stay
        // with that instantiation: redirecting a signature to a
        // sub-ascribed face of the same resource would sever it
        // from the plain export a consumer pairs it with. The
        // context records the set so instantiation can exempt these
        // roots from the tie (see `TransformContext::plain_exported`).
        let mut plain_exported: Vec<TypeId> = Vec::new();
        for instr in program {
            if let GenericsReplaceDSL::ExportTypeEq(_, ty) = instr {
                let mut root = *ty;
                while let Ok(Type::Generic(Generic {
                    bound: GenericBound::Eq(inner),
                    ..
                })) = validator.get_type(root)
                {
                    root = *inner;
                }
                if validator.sub_face_root(root).is_none() {
                    plain_exported.push(root);
                    env.unified.mark_plain_exported(root);
                }
            }
        }
        // Seed the ties between `(sub resource)` export faces and
        // the resources they abstract before running the program, so
        // references to an underlying resource elsewhere in the
        // program instantiate as the face's resource regardless of
        // the order the exports are declared in. A resource the
        // context has already substituted -- an imported resource
        // instantiated by an actual argument -- is exempt from the
        // tie: its face denotes the substituted type itself, so the
        // face is recorded as denoting it up front; otherwise a
        // later reference to the face (a plain re-export, a
        // signature) would instantiate it as a fresh generic
        // unrelated to the resource the face export itself denotes.
        for instr in program {
            if let GenericsReplaceDSL::ExportTypeSub(_, face) = instr {
                if let Some(root) = validator.sub_face_root(*face) {
                    if let Some(actual) = env.unified.get(root) {
                        env.unified.insert(*face, actual);
                    } else {
                        if !plain_exported.contains(&root) {
                            env.unified.seed_tie(root, *face);
                        }
                        env.unified.seed_tie(*face, *face);
                    }
                }
            }
        }
        tracing::trace!("{program:?}");
        for instr in program {
            Self::evaluate_one(instr, validator, &mut env)?
        }
        Ok(env.result)
    }
}
