use crate::component::linker::{AsyncHostFn, CoreExportBinding};
use crate::component::{ComponentError, ComponentProgram, ComponentValue};
use crate::{run_module_function, ResultValue, Store, VMResult, WasmValue};
use std::collections::HashMap;

#[derive(Clone)]
pub(crate) enum ResolvedCallable {
    Host(AsyncHostFn),
    Core {
        instance: crate::common::InstanceHandle,
        export_name: String,
    },
}

#[derive(Clone)]
pub struct ComponentInstance {
    exports: HashMap<String, ResolvedCallable>,
    pub(crate) _program: ComponentProgram,
}

impl ComponentInstance {
    pub(crate) fn new(
        program: ComponentProgram,
        exports: HashMap<String, ResolvedCallable>,
    ) -> Self {
        Self {
            exports,
            _program: program,
        }
    }

    pub async fn call(
        &self,
        store: &mut Store,
        name: &str,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        let callable = self
            .exports
            .get(name)
            .cloned()
            .ok_or_else(|| ComponentError::ExportNotFound(name.to_string()))?;

        match callable {
            ResolvedCallable::Host(f) => f(store, args).await,
            ResolvedCallable::Core {
                instance,
                export_name,
            } => call_core_export(&instance, store, &export_name, args).await,
        }
    }
}

impl From<CoreExportBinding> for ResolvedCallable {
    fn from(value: CoreExportBinding) -> Self {
        Self::Core {
            instance: value.instance,
            export_name: value.export_name,
        }
    }
}

fn component_args_to_result_value(args: &[ComponentValue]) -> Result<ResultValue, ComponentError> {
    let args = args
        .iter()
        .map(|arg| match arg {
            ComponentValue::I32(v) => WasmValue::I32(*v),
            ComponentValue::I64(v) => WasmValue::I64(*v),
            ComponentValue::F32(v) => WasmValue::F32(*v),
            ComponentValue::F64(v) => WasmValue::F64(*v),
        })
        .collect::<Vec<_>>();
    Ok(ResultValue::new(args))
}

fn result_value_to_component_values(
    result: &ResultValue,
) -> Result<Vec<ComponentValue>, ComponentError> {
    result
        .iter()
        .map(|value| match value {
            WasmValue::I32(v) => Ok(ComponentValue::I32(*v)),
            WasmValue::I64(v) => Ok(ComponentValue::I64(*v)),
            WasmValue::F32(v) => Ok(ComponentValue::F32(*v)),
            WasmValue::F64(v) => Ok(ComponentValue::F64(*v)),
            WasmValue::V128(_) => Err(ComponentError::Runtime(
                "v128 result is not supported in component runtime".to_owned(),
            )),
            WasmValue::FuncRef(_) => Err(ComponentError::Runtime(
                "funcref result is not supported in component runtime".to_owned(),
            )),
            WasmValue::ExternRef(_) => Err(ComponentError::Runtime(
                "externref result is not supported in component runtime".to_owned(),
            )),
        })
        .collect()
}

async fn call_core_export(
    instance: &crate::common::InstanceHandle,
    store: &mut Store,
    export_name: &str,
    args: &[ComponentValue],
) -> Result<Vec<ComponentValue>, ComponentError> {
    let args = component_args_to_result_value(args)?;
    let result = run_module_function(instance, store, export_name, &args).await;
    match result {
        VMResult::Success(values) => result_value_to_component_values(&values),
        VMResult::Unreachable => Err(ComponentError::Runtime(format!(
            "core export '{}' trapped: unreachable",
            export_name
        ))),
        VMResult::StackOverflow => Err(ComponentError::Runtime(format!(
            "core export '{}' trapped: stack overflow",
            export_name
        ))),
        VMResult::MemoryIndexOutOfRange => Err(ComponentError::Runtime(format!(
            "core export '{}' trapped: memory index out of range",
            export_name
        ))),
        VMResult::TableIndexOutOfRange => Err(ComponentError::Runtime(format!(
            "core export '{}' trapped: table index out of range",
            export_name
        ))),
        VMResult::CallIndirectInvalidType => Err(ComponentError::Runtime(format!(
            "core export '{}' trapped: call indirect invalid type",
            export_name
        ))),
        VMResult::TableUninitialized => Err(ComponentError::Runtime(format!(
            "core export '{}' trapped: table uninitialized",
            export_name
        ))),
        VMResult::Unlinkable => Err(ComponentError::Runtime(format!(
            "core export '{}' failed: unlinkable",
            export_name
        ))),
        VMResult::InvalidOperand => Err(ComponentError::Runtime(format!(
            "core export '{}' failed: invalid operand",
            export_name
        ))),
    }
}
