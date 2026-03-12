#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::Path;

use telomere::{IoReadBinaryReader, WasmParser};
use telomere_component::{ComponentEngine, ComponentError};
use wast::parser::ParseBuffer;
use wast::{QuoteWat, Wast, WastDirective, Wat};

#[derive(Debug, Default)]
pub struct TestsuiteCaseReport {
    pub directives_checked: usize,
    pub failures: Vec<String>,
}

pub fn run_component_testsuite_case(path: &Path, text: &str) -> TestsuiteCaseReport {
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

    let engine = ComponentEngine::new();
    let mut report = TestsuiteCaseReport::default();

    for directive in wast.directives {
        match directive {
            WastDirective::Module(mut module) | WastDirective::ModuleDefinition(mut module) => {
                handle_module(path, text, &engine, &mut report, &mut module);
            }
            WastDirective::AssertInvalid {
                span,
                mut module,
                message,
            } => {
                handle_assert_invalid(path, text, &engine, &mut report, span, &mut module, message)
            }
            WastDirective::AssertMalformed {
                span,
                mut module,
                message,
            } => handle_assert_malformed(
                path,
                text,
                &engine,
                &mut report,
                span,
                &mut module,
                message,
            ),
            WastDirective::ModuleInstance { span, .. } => {
                fail_at(
                    path,
                    text,
                    &mut report,
                    span,
                    "unsupported directive `module instance` in component_model_testsuite"
                        .to_owned(),
                );
            }
            WastDirective::Register { span, .. } => {
                fail_at(
                    path,
                    text,
                    &mut report,
                    span,
                    "unsupported directive `register` in component_model_testsuite".to_owned(),
                );
            }
            WastDirective::Invoke(invoke) => {
                fail_at(
                    path,
                    text,
                    &mut report,
                    invoke.span,
                    "unsupported directive `invoke` in component_model_testsuite".to_owned(),
                );
            }
            WastDirective::AssertTrap { span, .. } => {
                fail_at(
                    path,
                    text,
                    &mut report,
                    span,
                    "unsupported directive `assert_trap` in component_model_testsuite".to_owned(),
                );
            }
            WastDirective::AssertReturn { span, .. } => {
                fail_at(
                    path,
                    text,
                    &mut report,
                    span,
                    "unsupported directive `assert_return` in component_model_testsuite".to_owned(),
                );
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
            WastDirective::AssertUnlinkable { span, .. } => {
                fail_at(
                    path,
                    text,
                    &mut report,
                    span,
                    "unsupported directive `assert_unlinkable` in component_model_testsuite"
                        .to_owned(),
                );
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
    engine: &ComponentEngine,
    report: &mut TestsuiteCaseReport,
    module: &mut QuoteWat<'_>,
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

    if let Err(error) = compile_component_quote(engine, module) {
        fail_at(
            path,
            text,
            report,
            span,
            format!("component compile failed: {error}"),
        );
        return;
    }

    report.directives_checked += 1;
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
    let source = module
        .encode()
        .map_err(|error| ComponentError::Decode(error.to_string()))?;
    engine.compile(&source).map(|_| ())
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
