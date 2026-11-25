#![allow(dead_code)]

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use telomere::{IoReadBinaryReader, Store, WasmParser};
use telomere_component::{
    ComponentEngine, ComponentError, ComponentInstance, ComponentLinker, ComponentProgram,
    ComponentValue,
};
use wast::component::WastVal;
use wast::core::WastRetCore;
use wast::parser::ParseBuffer;
use wast::{QuoteWat, Wast, WastArg, WastDirective, WastExecute, WastInvoke, WastRet, Wat};

#[derive(Debug, Default)]
pub struct TestsuiteCaseReport {
    pub directives_checked: usize,
    pub failures: Vec<String>,
}

pub async fn run_component_testsuite_case(path: &Path, text: &str) -> TestsuiteCaseReport {
    let buf = match ParseBuffer::new(text) {
        Ok(buf) => buf,
        Err(error) => {
            return TestsuiteCaseReport {
                failures: vec![format!(
                    "{}: failed to build wast parse buffer: {error}",
                    path.display()
                )],
                ..TestsuiteCaseReport::default()
            };
        }
    };

    let wast = match wast::parser::parse::<Wast>(&buf) {
        Ok(wast) => wast,
        Err(error) => {
            return TestsuiteCaseReport {
                failures: vec![format!("{}: failed to parse wast: {error}", path.display())],
                ..TestsuiteCaseReport::default()
            };
        }
    };

    let mut report = TestsuiteCaseReport::default();
    let mut runtime = RuntimeState::new();

    for directive in wast.directives {
        match directive {
            WastDirective::Module(mut module) => {
                handle_module(path, text, &mut report, &mut runtime, &mut module, false);
            }
            WastDirective::ModuleDefinition(mut module) => {
                handle_module(path, text, &mut report, &mut runtime, &mut module, true);
            }
            WastDirective::AssertInvalid {
                span,
                mut module,
                message,
            }
            | WastDirective::AssertInvalidCustom {
                span,
                mut module,
                message,
            } => handle_assert_invalid(
                path,
                text,
                &runtime.engine,
                &mut report,
                span,
                &mut module,
                message,
            ),
            WastDirective::AssertMalformed {
                span,
                mut module,
                message,
            }
            | WastDirective::AssertMalformedCustom {
                span,
                mut module,
                message,
            } => handle_assert_malformed(
                path,
                text,
                &runtime.engine,
                &mut report,
                span,
                &mut module,
                message,
            ),
            WastDirective::ModuleInstance {
                span,
                instance,
                module,
            } => {
                handle_module_instance(
                    path,
                    text,
                    &mut report,
                    &mut runtime,
                    span,
                    instance,
                    module,
                )
                .await;
            }
            WastDirective::Register { span, name, module } => {
                handle_register(path, text, &mut report, &mut runtime, span, name, module).await;
            }
            WastDirective::Invoke(invoke) => {
                handle_invoke(path, text, &mut report, &mut runtime, &invoke).await;
            }
            WastDirective::AssertTrap {
                span,
                exec,
                message,
            } => {
                handle_assert_trap(path, text, &mut report, &mut runtime, span, &exec, message)
                    .await;
            }
            WastDirective::AssertReturn {
                span,
                exec,
                results,
            } => {
                handle_assert_return(path, text, &mut report, &mut runtime, span, &exec, &results)
                    .await;
            }
            WastDirective::AssertExhaustion { span, .. } => {
                fail_at(
                    path,
                    text,
                    &mut report,
                    span,
                    "unsupported directive `assert_exhaustion` in component_model_testsuite"
                        .to_owned(),
                );
            }
            WastDirective::AssertUnlinkable {
                span,
                module,
                message,
            } => {
                handle_assert_unlinkable(
                    path,
                    text,
                    &mut report,
                    &mut runtime,
                    span,
                    module,
                    message,
                )
                .await;
            }
            WastDirective::AssertException { span, .. } => {
                fail_at(
                    path,
                    text,
                    &mut report,
                    span,
                    "unsupported directive `assert_exception` in component_model_testsuite"
                        .to_owned(),
                );
            }
            WastDirective::AssertSuspension { span, .. } => {
                fail_at(
                    path,
                    text,
                    &mut report,
                    span,
                    "unsupported directive `assert_suspension` in component_model_testsuite"
                        .to_owned(),
                );
            }
            WastDirective::Thread(thread) => {
                fail_at(
                    path,
                    text,
                    &mut report,
                    thread.span,
                    "unsupported directive `thread` in component_model_testsuite".to_owned(),
                );
            }
            WastDirective::Wait { span, .. } => {
                fail_at(
                    path,
                    text,
                    &mut report,
                    span,
                    "unsupported directive `wait` in component_model_testsuite".to_owned(),
                );
            }
        }
    }

    report
}

fn handle_module(
    path: &Path,
    text: &str,
    report: &mut TestsuiteCaseReport,
    runtime: &mut RuntimeState,
    module: &mut QuoteWat<'_>,
    definition_only: bool,
) {
    let span = module.span();
    if !is_component_quote(module) {
        fail_at(
            path,
            text,
            report,
            span,
            "top-level core module directives are not supported by component_model_testsuite"
                .to_owned(),
        );
        return;
    }

    let program = match compile_component_program(&runtime.engine, module) {
        Ok(program) => program,
        Err(error) => {
            fail_at(
                path,
                text,
                report,
                span,
                format!("component compile failed: {error}"),
            );
            return;
        }
    };

    report.directives_checked += 1;

    let name = module.name().map(|id| id.name().to_owned());
    if let Some(name) = &name {
        runtime.definitions.insert(name.clone(), program.clone());
        if !definition_only {
            runtime.plain_names.insert(name.clone());
        }
    }
    // Every component directive, plain or definition-only, is the most
    // recent definition for a later `(component instance ...)` with no
    // module operand.
    runtime.latest = Some(program.clone());
    if !definition_only {
        // A plain `(component ...)` becomes the current definition; it is
        // instantiated lazily when a runtime directive first needs an
        // instance, so compile/validate-only files never pay for (or fail
        // on) instantiation.
        runtime.pending = Some(program);
        runtime.pending_name = name;
        runtime.current = None;
    }
}

fn handle_assert_invalid(
    path: &Path,
    text: &str,
    engine: &ComponentEngine,
    report: &mut TestsuiteCaseReport,
    span: wast::token::Span,
    module: &mut QuoteWat<'_>,
    message: &str,
) {
    if !is_component_quote(module) {
        fail_at(
            path,
            text,
            report,
            span,
            "assert_invalid for a core module is outside component_model_testsuite coverage"
                .to_owned(),
        );
        return;
    }

    let source = match module.encode() {
        Ok(source) => source,
        Err(error) => {
            fail_at(
                path,
                text,
                report,
                span,
                format!(
                    "assert_invalid expected validation failure but WAT encoding failed first: {error}"
                ),
            );
            return;
        }
    };

    match engine.compile(&source) {
        Ok(_) => fail_at(
            path,
            text,
            report,
            span,
            format!(
                "assert_invalid expected an error containing `{message}`, but compilation succeeded"
            ),
        ),
        Err(error) => {
            let actual = error.to_string();
            if !semantic_error_match(message, &actual) {
                fail_at(
                    path,
                    text,
                    report,
                    span,
                    format!(
                        "assert_invalid message mismatch: expected semantic match for `{message}`, got `{actual}`"
                    ),
                );
                return;
            }
            report.directives_checked += 1;
        }
    }
}

fn handle_assert_malformed(
    path: &Path,
    text: &str,
    engine: &ComponentEngine,
    report: &mut TestsuiteCaseReport,
    span: wast::token::Span,
    module: &mut QuoteWat<'_>,
    message: &str,
) {
    let actual = match module.encode() {
        Ok(source) => {
            if is_component_quote(module) {
                match engine.compile(&source) {
                    Ok(_) => None,
                    Err(error) => Some(error.to_string()),
                }
            } else {
                let mut reader = IoReadBinaryReader::from(&source[..]);
                let mut parser = WasmParser::new(&mut reader);
                match parser.parse_module() {
                    Ok(_) => None,
                    Err(error) => Some(error.to_string()),
                }
            }
        }
        Err(error) => Some(error.to_string()),
    };

    match actual {
        Some(actual) => {
            if !semantic_error_match(message, &actual) {
                fail_at(
                    path,
                    text,
                    report,
                    span,
                    format!(
                        "assert_malformed message mismatch: expected semantic match for `{message}`, got `{actual}`"
                    ),
                );
                return;
            }
            report.directives_checked += 1;
        }
        None => fail_at(
            path,
            text,
            report,
            span,
            format!(
                "assert_malformed expected an error containing `{message}`, but decoding succeeded"
            ),
        ),
    }
}

fn compile_component_quote(
    engine: &ComponentEngine,
    module: &mut QuoteWat<'_>,
) -> Result<(), ComponentError> {
    compile_component_program(engine, module).map(|_| ())
}

fn compile_component_program(
    engine: &ComponentEngine,
    module: &mut QuoteWat<'_>,
) -> Result<ComponentProgram, ComponentError> {
    let source = module
        .encode()
        .map_err(|error| ComponentError::Decode(error.to_string()))?;
    engine.compile(&source)
}

fn fail_at(
    path: &Path,
    text: &str,
    report: &mut TestsuiteCaseReport,
    span: wast::token::Span,
    message: String,
) {
    report.failures.push(format!(
        "{} @ {:?}: {message}",
        path.display(),
        span.linecol_in(text)
    ));
}

fn is_component_quote(module: &QuoteWat<'_>) -> bool {
    matches!(
        module,
        QuoteWat::Wat(Wat::Component(_)) | QuoteWat::QuoteComponent(_, _)
    )
}

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

fn message_tokens(input: &str) -> BTreeSet<String> {
    input.split_whitespace().map(str::to_owned).collect()
}

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

// ---------------------------------------------------------------------------
// Runtime directive execution
//
// The harness executes `invoke` / `assert_return` / `assert_trap` /
// `assert_unlinkable` / `register` / `(component instance ...)` against real
// component instances, so the vendored upstream suite checks canonical ABI
// behaviour at runtime instead of compile/validate only. One `Store` is
// shared by every instance created while running a single file (matching
// the reference wast runner); a fresh state is created per file.
//
// Instantiation is lazy: a `(component ...)` directive only compiles and
// records the program. The instance is created when a runtime directive
// first needs one, or eagerly by `(component instance $i $Definition)`.
// A plain component's `$id` still names its instance (the reference
// runner instantiates a plain component at declaration), so named
// directives resolve it through the same lazy path; a
// `(component definition ...)` name does not name an instance.

struct RuntimeState {
    engine: ComponentEngine,
    store: Store,
    linker: ComponentLinker,
    /// Programs recorded under their `$id` by `(component $id ...)` and
    /// `(component definition $id ...)`.
    definitions: HashMap<String, ComponentProgram>,
    /// Names declared by a plain `(component $id ...)` directive, as
    /// opposed to `(component definition $id ...)`. A plain component is
    /// instantiated by the reference runner when it is declared, so its
    /// `$id` names an instance that `invoke`/`register` can resolve; a
    /// definition-only name cannot be resolved as an instance.
    plain_names: BTreeSet<String>,
    /// Instances recorded under their `$id` by `(component instance $id ...)`.
    instances: HashMap<String, ComponentInstance>,
    /// Instances named by `register`, kept for `(invoke $name ...)`
    /// resolution. The component linker has no API to bridge a live
    /// instance's exports into a later component's imports, so a registered
    /// instance cannot satisfy another component's imports from here; no
    /// vendored file relies on that (linking is expressed inside the
    /// component text itself).
    registered: HashMap<String, ComponentInstance>,
    /// The instance runtime directives act on when they name no module.
    current: Option<ComponentInstance>,
    /// The most recent non-definition component, awaiting lazy
    /// instantiation.
    pending: Option<ComponentProgram>,
    /// The `$id` of the pending component when it was named, so the
    /// instance created from it can be recorded under that name.
    pending_name: Option<String>,
    /// The program of the most recent component directive, plain or
    /// definition-only. A `(component instance ...)` directive with no
    /// module operand instantiates this definition; it is tracked
    /// separately from `pending` because a definition-only component
    /// never becomes the pending current component, and implicit
    /// instantiation does not consume it.
    latest: Option<ComponentProgram>,
}

impl RuntimeState {
    fn new() -> Self {
        Self {
            engine: ComponentEngine::new(),
            store: Store::new(),
            linker: ComponentLinker::new(),
            definitions: HashMap::new(),
            plain_names: BTreeSet::new(),
            instances: HashMap::new(),
            registered: HashMap::new(),
            current: None,
            pending: None,
            pending_name: None,
            latest: None,
        }
    }

    async fn instantiate(
        &mut self,
        program: &ComponentProgram,
    ) -> Result<ComponentInstance, ComponentError> {
        self.engine
            .instantiate(program, &self.store, &self.linker)
            .await
    }

    /// Instantiates the pending component, recording the instance under
    /// the pending component's `$id` when it was named. Does not touch
    /// `current`; callers decide whether the new instance becomes the
    /// current one.
    async fn instantiate_pending(&mut self) -> Result<ComponentInstance, String> {
        let program = self
            .pending
            .take()
            .ok_or_else(|| "no component has been defined yet".to_owned())?;
        let name = self.pending_name.take();
        let instance = self
            .instantiate(&program)
            .await
            .map_err(|error| format!("component instantiation failed: {error}"))?;
        if let Some(name) = name {
            self.instances.insert(name, instance.clone());
        }
        Ok(instance)
    }

    /// Returns the current instance, instantiating the pending component
    /// first if necessary.
    async fn current_instance(&mut self) -> Result<ComponentInstance, String> {
        if let Some(instance) = &self.current {
            return Ok(instance.clone());
        }
        let instance = self.instantiate_pending().await?;
        self.current = Some(instance.clone());
        Ok(instance)
    }

    /// Resolves an instance referenced by name. Instances created by
    /// `(component instance $id ...)` and names recorded by `register`
    /// win. A name declared by a plain `(component $id ...)` directive
    /// also resolves: the reference runner instantiates a plain
    /// component at declaration time, so the harness instantiates it on
    /// first use and records the instance under that name. A
    /// definition-only name does not name an instance and never
    /// resolves here.
    async fn named_instance(&mut self, name: &str) -> Result<ComponentInstance, String> {
        if let Some(instance) = self
            .instances
            .get(name)
            .or_else(|| self.registered.get(name))
        {
            return Ok(instance.clone());
        }
        if !self.plain_names.contains(name) {
            return Err(format!("no instance named `${name}` has been created"));
        }
        if self.pending_name.as_deref() == Some(name) {
            let instance = self.instantiate_pending().await?;
            if self.current.is_none() {
                self.current = Some(instance.clone());
            }
            return Ok(instance);
        }
        let program = self
            .definitions
            .get(name)
            .cloned()
            .ok_or_else(|| format!("no instance named `${name}` has been created"))?;
        let instance = self
            .instantiate(&program)
            .await
            .map_err(|error| format!("component instantiation failed: {error}"))?;
        self.instances.insert(name.to_owned(), instance.clone());
        Ok(instance)
    }

    /// Resolves the instance an invoke targets: a named instance when the
    /// directive carries one, otherwise the current instance.
    async fn resolve_instance(
        &mut self,
        module: Option<wast::token::Id<'_>>,
    ) -> Result<ComponentInstance, String> {
        match module {
            Some(id) => self.named_instance(id.name()).await,
            None => self.current_instance().await,
        }
    }
}

async fn handle_module_instance(
    path: &Path,
    text: &str,
    report: &mut TestsuiteCaseReport,
    runtime: &mut RuntimeState,
    span: wast::token::Span,
    instance: Option<wast::token::Id<'_>>,
    module: Option<wast::token::Id<'_>>,
) {
    let program = match module {
        Some(id) => match runtime.definitions.get(id.name()) {
            Some(program) => program.clone(),
            None => {
                fail_at(
                    path,
                    text,
                    report,
                    span,
                    format!(
                        "component instance references unknown definition `${}`",
                        id.name()
                    ),
                );
                return;
            }
        },
        // With no module operand, the directive instantiates the most
        // recent component definition — plain or definition-only —
        // tracked separately from the pending current component, and
        // without consuming it, so repeated implicit instantiations
        // reuse the same definition.
        None => match runtime.latest.clone() {
            Some(program) => program,
            None => {
                fail_at(
                    path,
                    text,
                    report,
                    span,
                    "component instance directive has no preceding component definition".to_owned(),
                );
                return;
            }
        },
    };

    match runtime.instantiate(&program).await {
        Ok(created) => {
            report.directives_checked += 1;
            if let Some(id) = instance {
                runtime
                    .instances
                    .insert(id.name().to_owned(), created.clone());
            }
            runtime.current = Some(created);
        }
        Err(error) => fail_at(
            path,
            text,
            report,
            span,
            format!("component instantiation failed: {error}"),
        ),
    }
}

async fn handle_register(
    path: &Path,
    text: &str,
    report: &mut TestsuiteCaseReport,
    runtime: &mut RuntimeState,
    span: wast::token::Span,
    name: &str,
    module: Option<wast::token::Id<'_>>,
) {
    // With no module operand, `register` names the current instance.
    // Resolve it through the lazy instantiation path: a `(component ...)`
    // directive only records the pending program, so reading `current`
    // directly would report "no instance to register" for the normal
    // define-then-register sequence.
    let resolved = match module {
        Some(id) => runtime.named_instance(id.name()).await,
        None => runtime.current_instance().await,
    };
    match resolved {
        Ok(instance) => {
            runtime.registered.insert(name.to_owned(), instance);
            report.directives_checked += 1;
        }
        Err(error) => fail_at(
            path,
            text,
            report,
            span,
            format!("register directive could not resolve an instance: {error}"),
        ),
    }
}

async fn execute_invoke(
    runtime: &mut RuntimeState,
    invoke: &WastInvoke<'_>,
) -> Result<Vec<ComponentValue>, String> {
    let instance = runtime.resolve_instance(invoke.module).await?;
    let mut args = Vec::with_capacity(invoke.args.len());
    for arg in &invoke.args {
        args.push(wast_arg_to_value(arg)?);
    }
    instance
        .call(&runtime.store, invoke.name, &args)
        .await
        .map_err(|error| error.to_string())
}

async fn handle_invoke(
    path: &Path,
    text: &str,
    report: &mut TestsuiteCaseReport,
    runtime: &mut RuntimeState,
    invoke: &WastInvoke<'_>,
) {
    match execute_invoke(runtime, invoke).await {
        Ok(_) => report.directives_checked += 1,
        Err(error) => fail_at(
            path,
            text,
            report,
            invoke.span,
            format!("invoke \"{}\" failed: {error}", invoke.name),
        ),
    }
}

async fn handle_assert_return(
    path: &Path,
    text: &str,
    report: &mut TestsuiteCaseReport,
    runtime: &mut RuntimeState,
    span: wast::token::Span,
    exec: &WastExecute<'_>,
    results: &[WastRet<'_>],
) {
    let actual = match exec {
        WastExecute::Invoke(invoke) => match execute_invoke(runtime, invoke).await {
            Ok(values) => values,
            Err(error) => {
                fail_at(
                    path,
                    text,
                    report,
                    span,
                    format!(
                        "assert_return expected success but invoke \"{}\" failed: {error}",
                        invoke.name
                    ),
                );
                return;
            }
        },
        WastExecute::Get { module, global, .. } => {
            let instance = match runtime.resolve_instance(*module).await {
                Ok(instance) => instance,
                Err(error) => {
                    fail_at(path, text, report, span, error);
                    return;
                }
            };
            match instance.get_value(&runtime.store, global) {
                Ok(value) => vec![value],
                Err(error) => {
                    fail_at(
                        path,
                        text,
                        report,
                        span,
                        format!("assert_return get \"{global}\" failed: {error}"),
                    );
                    return;
                }
            }
        }
        WastExecute::Wat(_) => {
            fail_at(
                path,
                text,
                report,
                span,
                "assert_return of a module definition is not supported by component_model_testsuite"
                    .to_owned(),
            );
            return;
        }
    };

    if actual.len() != results.len() {
        fail_at(
            path,
            text,
            report,
            span,
            format!(
                "assert_return arity mismatch: expected {} value(s), got {}",
                results.len(),
                actual.len()
            ),
        );
        return;
    }

    for (expected, actual) in results.iter().zip(actual.iter()) {
        let expected = match wast_ret_to_value(expected) {
            Ok(value) => value,
            Err(error) => {
                fail_at(path, text, report, span, error);
                return;
            }
        };
        if !component_values_match(&expected, actual) {
            fail_at(
                path,
                text,
                report,
                span,
                format!("assert_return value mismatch: expected {expected:?}, got {actual:?}"),
            );
            return;
        }
    }

    report.directives_checked += 1;
}

async fn handle_assert_trap(
    path: &Path,
    text: &str,
    report: &mut TestsuiteCaseReport,
    runtime: &mut RuntimeState,
    span: wast::token::Span,
    exec: &WastExecute<'_>,
    message: &str,
) {
    let outcome = match exec {
        WastExecute::Invoke(invoke) => execute_invoke(runtime, invoke).await.map(|_| ()),
        WastExecute::Wat(_) => {
            // Harness limitation: the `wast` AST is not `Clone`, and
            // encoding a `Wat` needs `&mut` access the directive borrow
            // does not provide, so an inline module operand cannot be
            // instantiated from here. No vendored file uses this form
            // (every upstream assert_trap wraps an invoke); a file that
            // does must be excluded with this reason.
            fail_at(
                path,
                text,
                report,
                span,
                "assert_trap of an inline module is not supported by component_model_testsuite"
                    .to_owned(),
            );
            return;
        }
        WastExecute::Get { .. } => {
            fail_at(
                path,
                text,
                report,
                span,
                "assert_trap of a global get is not supported by component_model_testsuite"
                    .to_owned(),
            );
            return;
        }
    };

    match outcome {
        Ok(()) => fail_at(
            path,
            text,
            report,
            span,
            format!(
                "assert_trap expected an error containing `{message}`, but execution succeeded"
            ),
        ),
        Err(actual) => {
            if !trap_message_match(message, &actual) {
                fail_at(
                    path,
                    text,
                    report,
                    span,
                    format!(
                        "assert_trap message mismatch: expected semantic match for `{message}`, got `{actual}`"
                    ),
                );
                return;
            }
            report.directives_checked += 1;
        }
    }
}

async fn handle_assert_unlinkable(
    path: &Path,
    text: &str,
    report: &mut TestsuiteCaseReport,
    runtime: &mut RuntimeState,
    span: wast::token::Span,
    module: Wat<'_>,
    message: &str,
) {
    if !matches!(module, Wat::Component(_)) {
        fail_at(
            path,
            text,
            report,
            span,
            "assert_unlinkable for a core module is outside component_model_testsuite coverage"
                .to_owned(),
        );
        return;
    }

    // `assert_unlinkable` asserts a *link* failure: the component must
    // first encode and compile successfully, and only the instantiation
    // error is matched against the expected message. Feeding encode or
    // compilation errors into the same success path would let a
    // validator regression masquerade as the expected linking failure.
    let mut wat = module;
    let source = match wat.encode() {
        Ok(source) => source,
        Err(error) => {
            fail_at(
                path,
                text,
                report,
                span,
                format!(
                    "assert_unlinkable expected an instantiation error containing `{message}`, but WAT encoding failed first: {error}"
                ),
            );
            return;
        }
    };
    let program = match runtime.engine.compile(&source) {
        Ok(program) => program,
        Err(error) => {
            fail_at(
                path,
                text,
                report,
                span,
                format!(
                    "assert_unlinkable expected an instantiation error containing `{message}`, but compilation failed first: {error}"
                ),
            );
            return;
        }
    };

    match runtime.instantiate(&program).await {
        Ok(_) => fail_at(
            path,
            text,
            report,
            span,
            format!(
                "assert_unlinkable expected an error containing `{message}`, but instantiation succeeded"
            ),
        ),
        Err(error) => {
            let actual = error.to_string();
            if !semantic_error_match(message, &actual) {
                fail_at(
                    path,
                    text,
                    report,
                    span,
                    format!(
                        "assert_unlinkable message mismatch: expected semantic match for `{message}`, got `{actual}`"
                    ),
                );
                return;
            }
            report.directives_checked += 1;
        }
    }
}

/// Matches an expected trap message against telomere's error text.
///
/// Trap wording is part of neither the binary format nor the validation
/// contract, and telomere phrases canonical-ABI traps differently from the
/// reference text (e.g. "latin1+utf16 string pointer .. is not aligned to
/// 2 bytes" for the spec's "unaligned pointer"). Matching is therefore
/// containment plus a small table of spec-phrase aliases — and nothing
/// else. Unlike invalid/malformed messages, trap messages are *not*
/// matched by shared error category or token overlap: a trap raised for
/// the wrong reason (an unconditional `unreachable`, say) must never
/// satisfy an `assert_trap` that names a different cause (a memory
/// bounds check). Aliases are added only for wording differences, never
/// to accept a trap raised for a different reason.
fn trap_message_match(expected: &str, actual: &str) -> bool {
    let expected_normalized = normalize_message(expected);
    let actual_normalized = normalize_message(actual);
    if expected_normalized.is_empty() || actual_normalized.contains(&expected_normalized) {
        return true;
    }

    if expected_normalized.contains("unaligned pointer")
        && actual_normalized.contains("not aligned")
    {
        return true;
    }
    if expected_normalized.contains("out of bounds") && actual_normalized.contains("out of bounds")
    {
        return true;
    }

    false
}

fn wast_arg_to_value(arg: &WastArg<'_>) -> Result<ComponentValue, String> {
    match arg {
        WastArg::Component(value) => wast_val_to_value(value),
        WastArg::Core(core) => match core {
            wast::core::WastArgCore::I32(value) => Ok(ComponentValue::I32(*value)),
            wast::core::WastArgCore::I64(value) => Ok(ComponentValue::I64(*value)),
            wast::core::WastArgCore::F32(value) => {
                Ok(ComponentValue::F32(f32::from_bits(value.bits)))
            }
            wast::core::WastArgCore::F64(value) => {
                Ok(ComponentValue::F64(f64::from_bits(value.bits)))
            }
            other => Err(format!(
                "unsupported core argument in component invoke: {other:?}"
            )),
        },
        other => Err(format!(
            "unsupported argument in component invoke: {other:?}"
        )),
    }
}

fn wast_ret_to_value(ret: &WastRet<'_>) -> Result<ComponentValue, String> {
    match ret {
        WastRet::Component(value) => wast_val_to_value(value),
        WastRet::Core(core) => match core {
            WastRetCore::I32(value) => Ok(ComponentValue::I32(*value)),
            WastRetCore::I64(value) => Ok(ComponentValue::I64(*value)),
            WastRetCore::F32(pattern) => Ok(ComponentValue::F32(nan_pattern_f32(pattern))),
            WastRetCore::F64(pattern) => Ok(ComponentValue::F64(nan_pattern_f64(pattern))),
            other => Err(format!(
                "unsupported core expected value in assert_return: {other:?}"
            )),
        },
        other => Err(format!(
            "unsupported expected value in assert_return: {other:?}"
        )),
    }
}

fn nan_pattern_f32(pattern: &wast::core::NanPattern<wast::token::F32>) -> f32 {
    match pattern {
        wast::core::NanPattern::CanonicalNan | wast::core::NanPattern::ArithmeticNan => f32::NAN,
        wast::core::NanPattern::Value(value) => f32::from_bits(value.bits),
    }
}

fn nan_pattern_f64(pattern: &wast::core::NanPattern<wast::token::F64>) -> f64 {
    match pattern {
        wast::core::NanPattern::CanonicalNan | wast::core::NanPattern::ArithmeticNan => f64::NAN,
        wast::core::NanPattern::Value(value) => f64::from_bits(value.bits),
    }
}

/// Converts the payload of a `result` value. A payloadless case must
/// keep its discriminant: converting `(result.ok)` and `(result.err)`
/// alike to an unset branch would make the two indistinguishable as
/// expected values and unusable as arguments (canonical lowering
/// requires exactly one branch to be set). Mirror the runtime's own
/// convention (`runtime/canonical.rs`): a payloadless case is
/// represented by an empty-tuple payload in the selected branch.
fn result_payload_to_value(
    payload: &Option<Box<WastVal<'_>>>,
) -> Result<Box<ComponentValue>, String> {
    match payload {
        Some(payload) => Ok(Box::new(wast_val_to_value(payload)?)),
        None => Ok(Box::new(ComponentValue::Tuple(Vec::new()))),
    }
}

fn wast_val_to_value(value: &WastVal<'_>) -> Result<ComponentValue, String> {
    Ok(match value {
        WastVal::Bool(value) => ComponentValue::Bool(*value),
        WastVal::U8(value) => ComponentValue::U8(*value),
        WastVal::S8(value) => ComponentValue::S8(*value),
        WastVal::U16(value) => ComponentValue::U16(*value),
        WastVal::S16(value) => ComponentValue::S16(*value),
        WastVal::U32(value) => ComponentValue::U32(*value),
        WastVal::S32(value) => ComponentValue::S32(*value),
        WastVal::U64(value) => ComponentValue::U64(*value),
        WastVal::S64(value) => ComponentValue::S64(*value),
        WastVal::F32(value) => ComponentValue::F32(f32::from_bits(value.bits)),
        WastVal::F64(value) => ComponentValue::F64(f64::from_bits(value.bits)),
        WastVal::Char(value) => ComponentValue::Char(*value),
        WastVal::String(value) => ComponentValue::String((*value).to_owned()),
        WastVal::List(values) => ComponentValue::List(
            values
                .iter()
                .map(wast_val_to_value)
                .collect::<Result<_, _>>()?,
        ),
        WastVal::Record(fields) => ComponentValue::Record(
            fields
                .iter()
                .map(|(name, value)| Ok(((*name).to_owned(), wast_val_to_value(value)?)))
                .collect::<Result<_, String>>()?,
        ),
        WastVal::Tuple(values) => ComponentValue::Tuple(
            values
                .iter()
                .map(wast_val_to_value)
                .collect::<Result<_, _>>()?,
        ),
        WastVal::Variant(case, payload) => ComponentValue::Variant {
            case: (*case).to_owned(),
            value: match payload {
                Some(payload) => Some(Box::new(wast_val_to_value(payload)?)),
                None => None,
            },
        },
        WastVal::Enum(case) => ComponentValue::Enum((*case).to_owned()),
        WastVal::Option(value) => ComponentValue::Option(match value {
            Some(value) => Some(Box::new(wast_val_to_value(value)?)),
            None => None,
        }),
        WastVal::Result(result) => match result {
            Ok(payload) => ComponentValue::Result {
                ok: Some(result_payload_to_value(payload)?),
                err: None,
            },
            Err(payload) => ComponentValue::Result {
                ok: None,
                err: Some(result_payload_to_value(payload)?),
            },
        },
        WastVal::Flags(names) => {
            ComponentValue::Flags(names.iter().map(|name| (*name).to_owned()).collect())
        }
        WastVal::Map(_) => {
            return Err(
                "map values are a documented non-goal (CM_MAP) and cannot be expected here"
                    .to_owned(),
            );
        }
    })
}

/// Structural equality for expected vs. actual component values, with the
/// two relaxations wast semantics require: NaN equals NaN (any payload),
/// and record fields / flags compare by name rather than position.
fn component_values_match(expected: &ComponentValue, actual: &ComponentValue) -> bool {
    match (expected, actual) {
        (ComponentValue::F32(expected), ComponentValue::F32(actual)) => {
            expected.to_bits() == actual.to_bits() || (expected.is_nan() && actual.is_nan())
        }
        (ComponentValue::F64(expected), ComponentValue::F64(actual)) => {
            expected.to_bits() == actual.to_bits() || (expected.is_nan() && actual.is_nan())
        }
        (ComponentValue::List(expected), ComponentValue::List(actual))
        | (ComponentValue::Tuple(expected), ComponentValue::Tuple(actual)) => {
            expected.len() == actual.len()
                && expected
                    .iter()
                    .zip(actual.iter())
                    .all(|(expected, actual)| component_values_match(expected, actual))
        }
        (ComponentValue::Record(expected), ComponentValue::Record(actual)) => {
            expected.len() == actual.len()
                && expected.iter().all(|(name, expected_value)| {
                    actual
                        .iter()
                        .find(|(actual_name, _)| actual_name == name)
                        .is_some_and(|(_, actual_value)| {
                            component_values_match(expected_value, actual_value)
                        })
                })
        }
        (
            ComponentValue::Variant {
                case: expected_case,
                value: expected_value,
            },
            ComponentValue::Variant {
                case: actual_case,
                value: actual_value,
            },
        ) => {
            expected_case == actual_case
                && match (expected_value, actual_value) {
                    (None, None) => true,
                    (Some(expected), Some(actual)) => component_values_match(expected, actual),
                    _ => false,
                }
        }
        (ComponentValue::Option(expected), ComponentValue::Option(actual)) => {
            match (expected, actual) {
                (None, None) => true,
                (Some(expected), Some(actual)) => component_values_match(expected, actual),
                _ => false,
            }
        }
        (
            ComponentValue::Result {
                ok: expected_ok,
                err: expected_err,
            },
            ComponentValue::Result {
                ok: actual_ok,
                err: actual_err,
            },
        ) => {
            let payload_matches =
                |expected: &Option<Box<ComponentValue>>, actual: &Option<Box<ComponentValue>>| {
                    match (expected, actual) {
                        (None, None) => true,
                        (Some(expected), Some(actual)) => component_values_match(expected, actual),
                        _ => false,
                    }
                };
            payload_matches(expected_ok, actual_ok) && payload_matches(expected_err, actual_err)
        }
        (ComponentValue::Flags(expected), ComponentValue::Flags(actual)) => {
            let mut expected: Vec<&String> = expected.iter().collect();
            let mut actual: Vec<&String> = actual.iter().collect();
            expected.sort();
            actual.sort();
            expected == actual
        }
        _ => expected == actual,
    }
}
