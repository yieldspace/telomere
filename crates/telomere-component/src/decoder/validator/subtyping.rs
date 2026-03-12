use super::*;

impl<'a> Validator<'a> {
    pub fn assert_type_ids_subtype_of(&self, child: TypeId, parent: TypeId) -> ParseResult<()> {
        if child == parent {
            return Ok(());
        }

        match (self.get_type(child)?, self.get_type(parent)?) {
            (Type::Component(_), Type::Component(_)) => {
                self.assert_component_type_ids_subtype_of(child, parent)
            }
            (Type::Instance(_), Type::Instance(_)) => {
                self.assert_instance_type_ids_subtype_of(child, parent)
            }
            (lhs, rhs) => lhs.assert_subtype_of(rhs, self),
        }
    }

    pub fn assert_component_type_ids_subtype_of(
        &self,
        child: TypeId,
        parent: TypeId,
    ) -> ParseResult<()> {
        let child = self.component_surface_summary(child)?;
        let parent = self.component_surface_summary(parent)?;

        if child.imports.len() > parent.imports.len() {
            Err(ComponentParseError::TypeMismatch(
                "import count mismatch".to_owned(),
            ))?
        }

        let mut parent_index = 0;
        for child_import in &child.imports {
            while let Some(parent_import) = parent.imports.get(parent_index) {
                if parent_import.name < child_import.name {
                    parent_index += 1;
                    continue;
                }
                break;
            }
            let Some(parent_import) = parent.imports.get(parent_index) else {
                Err(ComponentParseError::TypeMismatch(
                    "import name mismatch".to_owned(),
                ))?
            };
            if parent_import.name != child_import.name {
                Err(ComponentParseError::TypeMismatch(
                    "import name mismatch".to_owned(),
                ))?
            }
            match (&child_import.kind, &parent_import.kind) {
                (
                    ComponentImportKind::Type(ComponentImportType::Type { generic: child, .. }),
                    ComponentImportKind::Type(ComponentImportType::Type {
                        generic: parent, ..
                    }),
                ) => child.bound.assert_subtype_of(&parent.bound, self)?,
                (
                    ComponentImportKind::CoreModule(child),
                    ComponentImportKind::CoreModule(parent),
                ) if child == parent => {}
                _ => Err(ComponentParseError::TypeMismatch(
                    "import kind mismatch".to_owned(),
                ))?,
            }
        }

        if parent.exports.len() > child.exports.len() {
            Err(ComponentParseError::TypeMismatch(
                "export count mismatch".to_owned(),
            ))?
        }

        let mut child_index = 0;
        for parent_export in &parent.exports {
            while let Some(child_export) = child.exports.get(child_index) {
                if child_export.name < parent_export.name {
                    child_index += 1;
                    continue;
                }
                break;
            }
            let Some(child_export) = child.exports.get(child_index) else {
                Err(ComponentParseError::TypeMismatch(
                    "export name mismatch".to_owned(),
                ))?
            };
            if child_export.name != parent_export.name {
                Err(ComponentParseError::TypeMismatch(
                    "export name mismatch".to_owned(),
                ))?
            }
            parent_export
                .kind
                .assert_subtype_of(&child_export.kind, self)?;
        }

        Ok(())
    }

    pub fn assert_instance_type_ids_subtype_of(
        &self,
        child: TypeId,
        parent: TypeId,
    ) -> ParseResult<()> {
        let child = self.instance_surface_summary(child)?;
        let parent = self.instance_surface_summary(parent)?;

        if child.exports.len() < parent.exports.len() {
            Err(ComponentParseError::TypeMismatch(
                "instance export count".to_owned(),
            ))?
        }

        let mut child_index = 0;
        for parent_export in &parent.exports {
            while let Some(child_export) = child.exports.get(child_index) {
                if child_export.name < parent_export.name {
                    child_index += 1;
                    continue;
                }
                break;
            }
            let Some(child_export) = child.exports.get(child_index) else {
                Err(ComponentParseError::TypeMismatch(
                    "instance export mismatch".to_owned(),
                ))?
            };
            if child_export.name != parent_export.name {
                Err(ComponentParseError::TypeMismatch(
                    "instance export mismatch".to_owned(),
                ))?
            }
            child_export
                .kind
                .assert_subtype_of(&parent_export.kind, self)?;
        }

        Ok(())
    }

    pub(super) fn component_surface_summary(
        &self,
        type_id: TypeId,
    ) -> ParseResult<std::cell::Ref<'_, ComponentSurfaceSummary>> {
        if self.types.component_surface_summary(type_id).is_none() {
            let ty = self.get_component_type(type_id)?.clone();
            let mut imports = ty
                .imports
                .iter()
                .map(|(name, import)| ComponentImportEntry {
                    name: self.types.intern_name(name),
                    kind: match import {
                        ComponentImportType::CoreModule(module) => {
                            ComponentImportKind::CoreModule(module.clone())
                        }
                        _ => ComponentImportKind::Type(import.clone()),
                    },
                })
                .collect::<Vec<_>>();
            imports.sort_unstable_by_key(|entry| entry.name);

            let mut exports = ty
                .exports
                .iter()
                .map(|(name, export)| ComponentExportEntry {
                    name: self.types.intern_name(name),
                    kind: export.clone(),
                })
                .collect::<Vec<_>>();
            exports.sort_unstable_by_key(|entry| entry.name);

            self.types.set_component_surface_summary(
                type_id,
                ComponentSurfaceSummary {
                    imports: imports.into_boxed_slice(),
                    exports: exports.into_boxed_slice(),
                },
            );
        }

        self.types
            .component_surface_summary(type_id)
            .ok_or_else(|| {
                ComponentParseError::TypeMismatch("component surface summary is missing".to_owned())
            })
    }

    pub(super) fn instance_surface_summary(
        &self,
        type_id: TypeId,
    ) -> ParseResult<std::cell::Ref<'_, InstanceSurfaceSummary>> {
        if self.types.instance_surface_summary(type_id).is_none() {
            let ty = self.get_instance_type(type_id)?.clone();
            let mut exports = ty
                .exports
                .iter()
                .map(|(name, export)| InstanceExportEntry {
                    name: self.types.intern_name(name),
                    kind: export.clone(),
                })
                .collect::<Vec<_>>();
            exports.sort_unstable_by_key(|entry| entry.name);

            self.types.set_instance_surface_summary(
                type_id,
                InstanceSurfaceSummary {
                    exports: exports.into_boxed_slice(),
                },
            );
        }

        self.types.instance_surface_summary(type_id).ok_or_else(|| {
            ComponentParseError::TypeMismatch("instance surface summary is missing".to_owned())
        })
    }
}
