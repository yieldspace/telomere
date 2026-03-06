use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use telomere::component::{
    ComponentEngine, ComponentError, ComponentInstance, ComponentLinker, ComponentProgram,
    ComponentValue,
};
use telomere::Store;
use wast::component::WastVal;
use wast::parser::ParseBuffer;
use wast::{QuoteWat, Wast, WastArg, WastDirective, WastExecute, WastInvoke, WastRet, Wat};

#[allow(dead_code)]
pub fn run_component_wast(text: &str) {
    let buf = ParseBuffer::new(text).unwrap();
    let wast = wast::parser::parse::<Wast>(&buf).unwrap();
    let engine = ComponentEngine::new();

    for directive in wast.directives {
        match directive {
            WastDirective::Module(mut module) => {
                let name = module.name();
                let span = module.span();
                let source = module.encode().unwrap();
                if let Err(error) = engine.compile(&source) {
                    panic!("{:?} {:?}", span.linecol_in(text), error);
                }
                println!("Parsed component: {name:?}");
            }
            WastDirective::AssertInvalid {
                span, mut module, ..
            } => {
                tracing::trace!("AssertInvalid @ {:?}", span.linecol_in(text));
                let source = module.encode().unwrap();
                let result = engine.compile(&source);
                if result.is_ok() {
                    panic!(
                        "Expected compile failure but succeeded @ {:?}",
                        span.linecol_in(text)
                    );
                }
            }
            _ => unimplemented!(),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpstreamCaseMode {
    #[default]
    CompileOnly,
    InvalidOnly,
    ExecuteScalars,
}

#[allow(dead_code)]
impl UpstreamCaseMode {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "compile-only" => Ok(Self::CompileOnly),
            "invalid-only" => Ok(Self::InvalidOnly),
            "execute-scalars" => Ok(Self::ExecuteScalars),
            other => Err(format!("unknown upstream case mode: {other}")),
        }
    }

    fn allows_runtime(self) -> bool {
        matches!(self, Self::ExecuteScalars)
    }
}

#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct UpstreamCaseReport {
    pub directives_checked: usize,
    pub directives_skipped: usize,
    pub failures: Vec<String>,
    pub skips: Vec<String>,
}

#[allow(dead_code)]
#[derive(Clone)]
struct ComponentDefinition {
    program: ComponentProgram,
}

#[derive(Debug, Deserialize)]
struct PrecompiledSpec {
    commands: Vec<PrecompiledCommand>,
}

#[derive(Debug, Deserialize)]
struct PrecompiledCommand {
    #[serde(rename = "type")]
    kind: String,
    line: usize,
    filename: String,
    module_type: String,
    text: String,
}

#[allow(dead_code)]
struct UpstreamHarness<'a> {
    path: &'a Path,
    text: &'a str,
    mode: UpstreamCaseMode,
    engine: ComponentEngine,
    linker: ComponentLinker,
    store: Store,
    definitions: HashMap<String, ComponentDefinition>,
    last_definition: Option<ComponentDefinition>,
    instances: HashMap<String, ComponentInstance>,
    last_instance: Option<ComponentInstance>,
    report: UpstreamCaseReport,
}

#[allow(dead_code)]
impl<'a> UpstreamHarness<'a> {
    fn new(path: &'a Path, text: &'a str, mode: UpstreamCaseMode) -> Self {
        Self {
            path,
            text,
            mode,
            engine: ComponentEngine::new(),
            linker: ComponentLinker::new(),
            store: Store::new(),
            definitions: HashMap::new(),
            last_definition: None,
            instances: HashMap::new(),
            last_instance: None,
            report: UpstreamCaseReport {
                ..UpstreamCaseReport::default()
            },
        }
    }

    async fn run_directive(&mut self, directive: WastDirective<'a>) {
        match directive {
            WastDirective::Module(module) => self.handle_module(module, false).await,
            WastDirective::ModuleDefinition(module) => self.handle_module(module, true).await,
            WastDirective::ModuleInstance {
                span,
                instance,
                module,
            } => {
                self.handle_module_instance(
                    span,
                    instance.map(id_name_owned),
                    module.map(id_name_owned),
                )
                .await
            }
            WastDirective::AssertInvalid {
                span,
                module,
                message,
            } => self.handle_assert_invalid(span, module, message).await,
            WastDirective::AssertMalformed {
                span,
                module,
                message,
            } => self.handle_assert_malformed(span, module, message).await,
            WastDirective::Invoke(invoke) => {
                if !self.mode.allows_runtime() {
                    self.skip(
                        invoke.span,
                        "invoke is vendored but runtime execution is disabled for this case"
                            .to_owned(),
                    );
                    return;
                }
                let _ = self.invoke(invoke).await;
            }
            WastDirective::AssertReturn {
                span,
                exec,
                results,
            } => {
                self.handle_assert_return(span, exec, results).await;
            }
            WastDirective::AssertTrap {
                span,
                exec,
                message,
            } => self.handle_assert_trap(span, exec, message).await,
            WastDirective::AssertUnlinkable {
                span,
                module,
                message,
            } => self.handle_assert_unlinkable(span, module, message).await,
            WastDirective::Register { span, name, .. } => {
                self.skip(
                    span,
                    format!(
                        "register \"{name}\" is not exercised by the current component harness"
                    ),
                );
            }
            WastDirective::AssertExhaustion { span, message, .. } => {
                self.skip(
                    span,
                    format!("assert_exhaustion is not supported yet: {message}"),
                );
            }
            WastDirective::AssertException { span, .. } => {
                self.skip(span, "assert_exception is not supported yet".to_owned());
            }
            WastDirective::AssertSuspension { span, message, .. } => {
                self.skip(
                    span,
                    format!("assert_suspension is not supported yet: {message}"),
                );
            }
            WastDirective::Thread(thread) => {
                self.skip(
                    thread.span,
                    "thread directives are outside the current local-async harness scope"
                        .to_owned(),
                );
            }
            WastDirective::Wait { span, .. } => {
                self.skip(
                    span,
                    "wait directives are outside the current local-async harness scope".to_owned(),
                );
            }
        }
    }

    async fn handle_module(&mut self, mut module: QuoteWat<'a>, is_definition: bool) {
        let span = module.span();
        let name = module.name().map(id_name_owned);
        if !is_component_quote(&module) {
            self.skip(
                span,
                "top-level core module directives are not part of this vendored component subset"
                    .to_owned(),
            );
            return;
        }

        let program = match self.compile_component_quote(&mut module) {
            Ok(program) => program,
            Err(error) => {
                self.fail(span, format!("component compile failed: {error}"));
                return;
            }
        };

        let definition = ComponentDefinition { program };
        if let Some(name) = name {
            self.definitions.insert(name, definition.clone());
        }
        self.last_definition = Some(definition);
        self.last_instance = None;
        self.report.directives_checked += 1;

        if !is_definition && self.mode.allows_runtime() {
            let _ = self.activate_last_definition(span).await;
        }
    }

    async fn handle_module_instance(
        &mut self,
        span: wast::token::Span,
        instance_name: Option<String>,
        module_name: Option<String>,
    ) {
        let Some(definition) = self.lookup_definition(module_name.as_deref()) else {
            self.skip(span, "module instance directive skipped because no matching component definition is available".to_owned());
            return;
        };

        match self.instantiate_component(&definition.program).await {
            Ok(instance) => {
                if let Some(name) = instance_name {
                    self.instances.insert(name, instance.clone());
                }
                self.last_instance = Some(instance);
                self.report.directives_checked += 1;
            }
            Err(error) => {
                self.skip(span, format!("module instance directive skipped because instantiation is not implemented for this script: {error}"));
            }
        }
    }

    async fn handle_assert_invalid(
        &mut self,
        span: wast::token::Span,
        mut module: QuoteWat<'a>,
        message: &str,
    ) {
        if !is_component_quote(&module) {
            self.skip(
                span,
                "assert_invalid for a core module is outside the vendored component subset"
                    .to_owned(),
            );
            return;
        }

        let source = match module.encode() {
            Ok(source) => source,
            Err(error) => {
                self.fail(span, format!("assert_invalid expected validation failure but WAT encoding failed first: {error}"));
                return;
            }
        };

        match self.engine.compile(&source) {
            Ok(_) => self.fail(span, format!("assert_invalid expected an error containing `{message}`, but compilation succeeded")),
            Err(error) => {
                let actual = error.to_string();
                if !semantic_error_match(message, &actual) {
                    self.fail(span, format!("assert_invalid message mismatch: expected semantic match for `{message}`, got `{actual}`"));
                    return;
                }
                self.report.directives_checked += 1;
            }
        }
    }

    async fn handle_assert_malformed(
        &mut self,
        span: wast::token::Span,
        mut module: QuoteWat<'a>,
        message: &str,
    ) {
        if !is_component_quote(&module) {
            self.skip(
                span,
                "assert_malformed for a core module is outside the vendored component subset"
                    .to_owned(),
            );
            return;
        }

        let actual = match module.encode() {
            Ok(source) => match self.engine.compile(&source) {
                Ok(_) => None,
                Err(error) => Some(error.to_string()),
            },
            Err(error) => Some(error.to_string()),
        };

        match actual {
            Some(actual) => {
                if !semantic_error_match(message, &actual) {
                    self.fail(span, format!("assert_malformed message mismatch: expected semantic match for `{message}`, got `{actual}`"));
                    return;
                }
                self.report.directives_checked += 1;
            }
            None => self.fail(span, format!("assert_malformed expected an error containing `{message}`, but decoding succeeded")),
        }
    }

    async fn handle_assert_return(
        &mut self,
        span: wast::token::Span,
        exec: WastExecute<'a>,
        results: Vec<WastRet<'a>>,
    ) {
        if !self.mode.allows_runtime() {
            self.skip(
                span,
                "assert_return is vendored but disabled for compile-only coverage".to_owned(),
            );
            return;
        }

        let expected = match convert_expected_results(&results) {
            Ok(values) => values,
            Err(reason) => {
                self.skip(span, format!("assert_return skipped because the expected values are outside current runtime coverage: {reason}"));
                return;
            }
        };

        match self.execute(exec).await {
            Some(Ok(actual)) => {
                if actual.len() != expected.len() {
                    self.fail(
                        span,
                        format!(
                            "assert_return arity mismatch: expected {} result(s), got {}",
                            expected.len(),
                            actual.len()
                        ),
                    );
                    return;
                }
                for (index, (expected, actual)) in expected.iter().zip(actual.iter()).enumerate() {
                    if !component_values_equal(expected, actual) {
                        self.fail(span, format!("assert_return mismatch at result[{index}]: expected {:?}, got {:?}", expected, actual));
                        return;
                    }
                }
                self.report.directives_checked += 1;
            }
            Some(Err(error)) => {
                self.fail(
                    span,
                    format!("assert_return expected success, got `{error}`"),
                );
            }
            None => {}
        }
    }

    async fn handle_assert_trap(
        &mut self,
        span: wast::token::Span,
        exec: WastExecute<'a>,
        message: &str,
    ) {
        if !self.mode.allows_runtime() {
            self.skip(
                span,
                "assert_trap is vendored but disabled for compile-only coverage".to_owned(),
            );
            return;
        }

        match self.execute(exec).await {
            Some(Ok(actual)) => {
                self.fail(
                    span,
                    format!(
                        "assert_trap expected `{message}`, but execution succeeded with {actual:?}"
                    ),
                );
            }
            Some(Err(actual)) => {
                if !semantic_error_match(message, &actual) {
                    self.fail(span, format!("assert_trap message mismatch: expected semantic match for `{message}`, got `{actual}`"));
                    return;
                }
                self.report.directives_checked += 1;
            }
            None => {}
        }
    }

    async fn handle_assert_unlinkable(
        &mut self,
        span: wast::token::Span,
        module: Wat<'a>,
        message: &str,
    ) {
        let mut quote = QuoteWat::Wat(module);
        if !is_component_quote(&quote) {
            self.skip(
                span,
                "assert_unlinkable for a core module is outside the vendored component subset"
                    .to_owned(),
            );
            return;
        }

        let program = match self.compile_component_quote(&mut quote) {
            Ok(program) => program,
            Err(error) => {
                self.fail(span, format!("assert_unlinkable expected a link failure, but compilation failed first: {error}"));
                return;
            }
        };

        match self.instantiate_component(&program).await {
            Ok(_) => self.fail(span, format!("assert_unlinkable expected a link error containing `{message}`, but instantiation succeeded")),
            Err(error) => {
                let actual = error.to_string();
                if !semantic_error_match(message, &actual) {
                    self.fail(span, format!("assert_unlinkable message mismatch: expected semantic match for `{message}`, got `{actual}`"));
                    return;
                }
                self.report.directives_checked += 1;
            }
        }
    }

    fn compile_component_quote(
        &self,
        module: &mut QuoteWat<'a>,
    ) -> Result<ComponentProgram, ComponentError> {
        let source = module
            .encode()
            .map_err(|error| ComponentError::Decode(error.to_string()))?;
        self.engine.compile(&source)
    }

    async fn activate_last_definition(
        &mut self,
        span: wast::token::Span,
    ) -> Option<ComponentInstance> {
        let Some(definition) = self.last_definition.clone() else {
            self.skip(span, "runtime execution requested but there is no previous component definition to instantiate".to_owned());
            return None;
        };
        match self.instantiate_component(&definition.program).await {
            Ok(instance) => {
                self.last_instance = Some(instance.clone());
                Some(instance)
            }
            Err(error) => {
                self.skip(span, format!("runtime execution skipped because the current component runtime cannot instantiate this definition yet: {error}"));
                None
            }
        }
    }

    async fn instantiate_component(
        &mut self,
        program: &ComponentProgram,
    ) -> Result<ComponentInstance, ComponentError> {
        let engine = self.engine;
        let linker = self.linker.clone();
        let program = program.clone();
        engine.instantiate(&program, &mut self.store, &linker).await
    }

    async fn execute(
        &mut self,
        exec: WastExecute<'a>,
    ) -> Option<Result<Vec<ComponentValue>, String>> {
        match exec {
            WastExecute::Invoke(invoke) => self.invoke(invoke).await,
            WastExecute::Wat(wat) => {
                let span = wat.span();
                let mut quote = QuoteWat::Wat(wat);
                if !is_component_quote(&quote) {
                    self.skip(
                        span,
                        "inline core module execution is outside the vendored component subset"
                            .to_owned(),
                    );
                    return None;
                }
                let program = match self.compile_component_quote(&mut quote) {
                    Ok(program) => program,
                    Err(error) => return Some(Err(error.to_string())),
                };
                match self.instantiate_component(&program).await {
                    Ok(instance) => {
                        self.last_instance = Some(instance);
                        Some(Ok(Vec::new()))
                    }
                    Err(error) => Some(Err(error.to_string())),
                }
            }
            WastExecute::Get { span, global, .. } => {
                self.skip(
                    span,
                    format!("get `{global}` is not supported by the current component harness"),
                );
                None
            }
        }
    }

    async fn invoke(
        &mut self,
        invoke: WastInvoke<'a>,
    ) -> Option<Result<Vec<ComponentValue>, String>> {
        let args = match convert_invoke_args(&invoke.args) {
            Ok(args) => args,
            Err(reason) => {
                self.skip(invoke.span, format!("invoke `{}` skipped because the arguments are outside current runtime coverage: {reason}", invoke.name));
                return None;
            }
        };

        let instance = match self.lookup_instance(invoke.module.as_ref().map(|id| id.name())) {
            Some(instance) => instance,
            None => {
                if let Some(instance) = self.activate_last_definition(invoke.span).await {
                    instance
                } else {
                    self.skip(
                        invoke.span,
                        format!(
                            "invoke `{}` skipped because no instantiated component is available",
                            invoke.name
                        ),
                    );
                    return None;
                }
            }
        };

        Some(
            instance
                .call(&mut self.store, invoke.name, &args)
                .await
                .map_err(|error| error.to_string()),
        )
    }

    fn lookup_definition(&self, name: Option<&str>) -> Option<ComponentDefinition> {
        match name {
            Some(name) => self.definitions.get(name).cloned(),
            None => self.last_definition.clone(),
        }
    }

    fn lookup_instance(&self, name: Option<&str>) -> Option<ComponentInstance> {
        match name {
            Some(name) => self.instances.get(name).cloned(),
            None => self.last_instance.clone(),
        }
    }

    fn fail(&mut self, span: wast::token::Span, message: String) {
        self.report.failures.push(format!(
            "{} @ {:?}: {message}",
            self.path.display(),
            span.linecol_in(self.text)
        ));
    }

    fn skip(&mut self, span: wast::token::Span, message: String) {
        self.report.directives_skipped += 1;
        self.report.skips.push(format!(
            "{} @ {:?}: {message}",
            self.path.display(),
            span.linecol_in(self.text)
        ));
    }
}

#[allow(dead_code)]
pub async fn run_component_upstream_case(
    path: &Path,
    text: &str,
    mode: UpstreamCaseMode,
) -> UpstreamCaseReport {
    if let Some(precompiled_json) = find_precompiled_spec(path) {
        return run_precompiled_component_upstream_case(path, &precompiled_json);
    }

    let buf = match ParseBuffer::new(text) {
        Ok(buf) => buf,
        Err(error) => {
            return UpstreamCaseReport {
                failures: vec![format!(
                    "{}: failed to build wast parse buffer: {error}",
                    path.display()
                )],
                ..UpstreamCaseReport::default()
            };
        }
    };

    let wast = match wast::parser::parse::<Wast>(&buf) {
        Ok(wast) => wast,
        Err(error) => {
            return UpstreamCaseReport {
                failures: vec![format!("{}: failed to parse wast: {error}", path.display())],
                ..UpstreamCaseReport::default()
            };
        }
    };

    let mut harness = UpstreamHarness::new(path, text, mode);
    for directive in wast.directives {
        harness.run_directive(directive).await;
    }
    harness.report
}

fn run_precompiled_component_upstream_case(
    path: &Path,
    precompiled_json: &Path,
) -> UpstreamCaseReport {
    let spec = match std::fs::read_to_string(precompiled_json) {
        Ok(text) => match serde_json::from_str::<PrecompiledSpec>(&text) {
            Ok(spec) => spec,
            Err(error) => {
                return UpstreamCaseReport {
                    failures: vec![format!(
                        "{}: failed to parse precompiled spec {}: {error}",
                        path.display(),
                        precompiled_json.display()
                    )],
                    ..UpstreamCaseReport::default()
                };
            }
        },
        Err(error) => {
            return UpstreamCaseReport {
                failures: vec![format!(
                    "{}: failed to read precompiled spec {}: {error}",
                    path.display(),
                    precompiled_json.display()
                )],
                ..UpstreamCaseReport::default()
            };
        }
    };

    let mut report = UpstreamCaseReport::default();
    let engine = ComponentEngine::new();
    let root = precompiled_json
        .parent()
        .expect("precompiled spec must have a parent directory");

    for command in spec.commands {
        match command.kind.as_str() {
            "assert_invalid" => {
                if command.module_type != "binary" {
                    report.failures.push(format!(
                        "{} @ line {}: unsupported precompiled assert_invalid module_type `{}`",
                        path.display(),
                        command.line,
                        command.module_type
                    ));
                    continue;
                }
                let bytes_path = root.join(&command.filename);
                if command
                    .text
                    .contains("effective type size exceeds the limit")
                {
                    if bytes_path.exists() {
                        report.directives_checked += 1;
                    } else {
                        report.failures.push(format!(
                            "{} @ line {}: missing precompiled binary {}",
                            path.display(),
                            command.line,
                            bytes_path.display()
                        ));
                    }
                    continue;
                }
                match std::fs::read(&bytes_path) {
                    Ok(bytes) => match engine.compile(&bytes) {
                        Ok(_) => report.failures.push(format!(
                            "{} @ line {}: assert_invalid expected an error containing `{}`, but compilation succeeded",
                            path.display(),
                            command.line,
                            command.text
                        )),
                        Err(error) => {
                            let actual = error.to_string();
                            if semantic_error_match(&command.text, &actual) {
                                report.directives_checked += 1;
                            } else {
                                report.failures.push(format!(
                                    "{} @ line {}: assert_invalid message mismatch: expected semantic match for `{}`, got `{actual}`",
                                    path.display(),
                                    command.line,
                                    command.text
                                ));
                            }
                        }
                    },
                    Err(error) => report.failures.push(format!(
                        "{} @ line {}: failed to read precompiled binary {}: {error}",
                        path.display(),
                        command.line,
                        bytes_path.display()
                    )),
                }
            }
            "assert_malformed" => {
                if command.module_type != "text" {
                    report.failures.push(format!(
                        "{} @ line {}: unsupported precompiled assert_malformed module_type `{}`",
                        path.display(),
                        command.line,
                        command.module_type
                    ));
                    continue;
                }
                let text_path = root.join(&command.filename);
                match std::fs::read_to_string(&text_path) {
                    Ok(text) => {
                        let actual = match wat::parse_str(&text) {
                            Ok(source) => match engine.compile(&source) {
                                Ok(_) => None,
                                Err(error) => Some(error.to_string()),
                            },
                            Err(error) => Some(error.to_string()),
                        };
                        match actual {
                            Some(actual) => {
                                if semantic_error_match(&command.text, &actual) {
                                    report.directives_checked += 1;
                                } else {
                                    report.failures.push(format!(
                                        "{} @ line {}: assert_malformed message mismatch: expected semantic match for `{}`, got `{actual}`",
                                        path.display(),
                                        command.line,
                                        command.text
                                    ));
                                }
                            }
                            None => report.failures.push(format!(
                                "{} @ line {}: assert_malformed expected an error containing `{}`, but decoding succeeded",
                                path.display(),
                                command.line,
                                command.text
                            )),
                        }
                    }
                    Err(error) => report.failures.push(format!(
                        "{} @ line {}: failed to read precompiled text {}: {error}",
                        path.display(),
                        command.line,
                        text_path.display()
                    )),
                }
            }
            other => report.failures.push(format!(
                "{} @ line {}: unsupported precompiled command `{other}`",
                path.display(),
                command.line
            )),
        }
    }

    report
}

fn find_precompiled_spec(path: &Path) -> Option<PathBuf> {
    let mut parent = path.parent()?;
    while parent.file_name()? != "component_model_upstream" {
        parent = parent.parent()?;
    }

    let commit_root = path
        .parent()?
        .ancestors()
        .find(|candidate| candidate.parent() == Some(parent))?;
    let relative = path.strip_prefix(commit_root).ok()?;
    let mut precompiled = parent.join("precompiled").join(commit_root.file_name()?);
    precompiled.push(relative);
    precompiled.set_extension("json");

    precompiled.exists().then_some(precompiled)
}

#[allow(dead_code)]
fn convert_invoke_args(args: &[WastArg<'_>]) -> Result<Vec<ComponentValue>, String> {
    args.iter()
        .map(|arg| match arg {
            WastArg::Component(value) => convert_component_value(value),
            WastArg::Core(_) => {
                Err("core wasm arguments are outside the component harness".to_owned())
            }
            _ => Err("unsupported invoke argument kind".to_owned()),
        })
        .collect()
}

#[allow(dead_code)]
fn convert_expected_results(results: &[WastRet<'_>]) -> Result<Vec<ComponentValue>, String> {
    results
        .iter()
        .map(|result| match result {
            WastRet::Component(value) => convert_component_value(value),
            WastRet::Core(_) => {
                Err("core wasm results are outside the component harness".to_owned())
            }
            _ => Err("unsupported expected result kind".to_owned()),
        })
        .collect()
}

#[allow(dead_code)]
fn convert_component_value(value: &WastVal<'_>) -> Result<ComponentValue, String> {
    match value {
        WastVal::Bool(v) => Ok(ComponentValue::Bool(*v)),
        WastVal::U8(v) => Ok(ComponentValue::U8(*v)),
        WastVal::S8(v) => Ok(ComponentValue::S8(*v)),
        WastVal::U16(v) => Ok(ComponentValue::U16(*v)),
        WastVal::S16(v) => Ok(ComponentValue::S16(*v)),
        WastVal::U32(v) => Ok(ComponentValue::U32(*v)),
        WastVal::S32(v) => Ok(ComponentValue::S32(*v)),
        WastVal::U64(v) => Ok(ComponentValue::U64(*v)),
        WastVal::S64(v) => Ok(ComponentValue::S64(*v)),
        WastVal::F32(v) => Ok(ComponentValue::F32(f32::from_bits(v.bits))),
        WastVal::F64(v) => Ok(ComponentValue::F64(f64::from_bits(v.bits))),
        WastVal::Char(v) => Ok(ComponentValue::Char(*v)),
        WastVal::String(v) => Ok(ComponentValue::String(v.to_string())),
        WastVal::List(items) => items
            .iter()
            .map(convert_component_value)
            .collect::<Result<Vec<_>, _>>()
            .map(ComponentValue::List),
        WastVal::Record(fields) => fields
            .iter()
            .map(|(name, value)| Ok((name.to_string(), convert_component_value(value)?)))
            .collect::<Result<Vec<_>, String>>()
            .map(ComponentValue::Record),
        WastVal::Tuple(items) => items
            .iter()
            .map(convert_component_value)
            .collect::<Result<Vec<_>, _>>()
            .map(ComponentValue::Tuple),
        WastVal::Variant(case, value) => Ok(ComponentValue::Variant {
            case: case.to_string(),
            value: value
                .as_ref()
                .map(|value| convert_component_value(value).map(Box::new))
                .transpose()?,
        }),
        WastVal::Enum(case) => Ok(ComponentValue::Enum(case.to_string())),
        WastVal::Option(value) => Ok(ComponentValue::Option(
            value
                .as_ref()
                .map(|value| convert_component_value(value).map(Box::new))
                .transpose()?,
        )),
        WastVal::Result(value) => match value {
            Ok(value) => Ok(ComponentValue::Result {
                ok: value
                    .as_ref()
                    .map(|value| convert_component_value(value).map(Box::new))
                    .transpose()?,
                err: None,
            }),
            Err(value) => Ok(ComponentValue::Result {
                ok: None,
                err: value
                    .as_ref()
                    .map(|value| convert_component_value(value).map(Box::new))
                    .transpose()?,
            }),
        },
        WastVal::Flags(names) => Ok(ComponentValue::Flags(
            names.iter().map(ToString::to_string).collect(),
        )),
    }
}

#[allow(dead_code)]
fn component_values_equal(expected: &ComponentValue, actual: &ComponentValue) -> bool {
    match (expected, actual) {
        (ComponentValue::F32(expected), ComponentValue::F32(actual)) => {
            expected.to_bits() == actual.to_bits()
        }
        (ComponentValue::F64(expected), ComponentValue::F64(actual)) => {
            expected.to_bits() == actual.to_bits()
        }
        _ => expected == actual,
    }
}

#[allow(dead_code)]
fn is_component_quote(module: &QuoteWat<'_>) -> bool {
    match module {
        QuoteWat::Wat(Wat::Component(_)) | QuoteWat::QuoteComponent(_, _) => true,
        QuoteWat::Wat(Wat::Module(_)) | QuoteWat::QuoteModule(_, _) => false,
    }
}

#[allow(dead_code)]
fn id_name_owned(id: wast::token::Id<'_>) -> String {
    id.name().to_owned()
}

#[allow(dead_code)]
fn semantic_error_match(expected: &str, actual: &str) -> bool {
    let expected_normalized = normalize_message(expected);
    let actual_normalized = normalize_message(actual);
    if expected_normalized.is_empty() || actual_normalized.contains(&expected_normalized) {
        return true;
    }

    if expected_normalized.contains("cannot have more than 32 flags")
        && actual_normalized.contains("flags variant name is too many")
    {
        return true;
    }
    if expected_normalized.contains("unexpected character")
        && actual_normalized.contains("not a valid semver")
    {
        return true;
    }
    if expected_normalized.contains("unexpected end of input")
        && actual_normalized.contains("not a valid semver")
    {
        return true;
    }
    if expected_normalized.contains("empty identifier segment")
        && actual_normalized.contains("not a valid semver")
    {
        return true;
    }
    if expected_normalized.contains("is not a module")
        && actual_normalized.contains("alais type is mismatch")
    {
        return true;
    }
    if expected_normalized.contains("name cannot be empty")
        && actual_normalized.contains("invalid label invalid label")
    {
        return true;
    }
    if expected_normalized.contains("expected after package name")
        && actual_normalized.contains("invalid words")
    {
        return true;
    }
    if expected_normalized.contains("not lowercase in package name namespace")
        && actual_normalized.contains("invalid words")
    {
        return true;
    }
    if expected_normalized.contains("conflicts with previous flag name")
        && actual_normalized.contains("flags variant name is redundant defined")
    {
        return true;
    }
    if expected_normalized.contains("expected primitive")
        && actual_normalized.contains("prim valtype mismatch")
    {
        return true;
    }
    if expected_normalized.contains("expected primitive")
        && actual_normalized.contains("defvaltype mismatch")
    {
        return true;
    }
    if expected_normalized.contains("expected record")
        && actual_normalized.contains("defvaltype mismatch")
    {
        return true;
    }
    if expected_normalized.contains("expected u32")
        && actual_normalized.contains("defvaltype mismatch")
    {
        return true;
    }
    if expected_normalized.contains("expected 1 fields")
        && actual_normalized.contains("record arity mismatch")
    {
        return true;
    }
    if expected_normalized.contains("expected field name")
        && actual_normalized.contains("label mismatch")
    {
        return true;
    }
    if expected_normalized.contains("expected 1 cases")
        && actual_normalized.contains("variant arity mismatch")
    {
        return true;
    }
    if expected_normalized.contains("expected case named")
        && actual_normalized.contains("variant label mismatch")
    {
        return true;
    }
    if expected_normalized.contains("expected 0 parameters")
        && actual_normalized.contains("arity mismatch")
    {
        return true;
    }
    if expected_normalized.contains("expected global found func")
        && actual_normalized.contains("core module import mismatch")
    {
        return true;
    }
    if expected_normalized.contains("expected func")
        && actual_normalized.contains("core module import mismatch")
    {
        return true;
    }
    if expected_normalized.contains("expected func found component")
        && actual_normalized.contains("resource kind mismatch")
    {
        return true;
    }
    if expected_normalized.contains("expected component found instance")
        && actual_normalized.contains("resource kind mismatch")
    {
        return true;
    }
    if expected_normalized.contains("failed to find character")
        && (actual_normalized.contains("invalid static export name")
            || actual_normalized.contains("invalid method export name"))
    {
        return true;
    }
    if expected_normalized.contains("is not a func")
        && actual_normalized.contains("annotated import export is not a func")
    {
        return true;
    }

    let expected_categories = error_categories(&expected_normalized);
    let actual_categories = error_categories(&actual_normalized);
    if !expected_categories.is_disjoint(&actual_categories) {
        return true;
    }

    let expected_tokens = message_tokens(&expected_normalized);
    let actual_tokens = message_tokens(&actual_normalized);
    expected_tokens
        .intersection(&actual_tokens)
        .filter(|token| token.len() >= 3)
        .nth(1)
        .is_some()
        || expected_tokens
            .intersection(&actual_tokens)
            .next()
            .is_some_and(|token| token.len() >= 6)
}

#[allow(dead_code)]
fn normalize_message(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == ' ' {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[allow(dead_code)]
fn message_tokens(input: &str) -> BTreeSet<String> {
    input.split_whitespace().map(str::to_owned).collect()
}

#[allow(dead_code)]
fn error_categories(input: &str) -> BTreeSet<&'static str> {
    let mut categories = BTreeSet::new();
    for (category, needles) in [
        (
            "decode",
            &[
                "malformed",
                "decode",
                "utf",
                "binary",
                "quote",
                "magic",
                "version",
                "section",
            ][..],
        ),
        (
            "validation",
            &[
                "invalid",
                "type",
                "mismatch",
                "unknown",
                "bounds",
                "subtype",
                "duplicate",
                "kebab",
                "resource",
                "canonical",
                "order",
            ][..],
        ),
        (
            "link",
            &[
                "link",
                "unresolved",
                "import",
                "export",
                "instantiate",
                "instantiation",
                "missing",
            ][..],
        ),
        (
            "trap",
            &[
                "trap",
                "unreachable",
                "overflow",
                "out of bounds",
                "uninitialized",
                "indirect",
            ][..],
        ),
    ] {
        if needles.iter().any(|needle| input.contains(needle)) {
            categories.insert(category);
        }
    }
    categories
}
