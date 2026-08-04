//! Measurement-only optimizer pipeline switch resolution.
//!
//! This module is compiled only with the `measure-switches` feature. The
//! resolved value is process-scoped so every library caller observes the same
//! switch state, while CLI callers can reject an invalid environment before a
//! measurement starts.

use std::{env, ffi::OsString, fmt, sync::OnceLock};

/// The environment variable that controls the measurement-only optimizer switch.
pub const ENVIRONMENT_VARIABLE: &str = "TELOMERE_OPTIMIZER";

const OFF_VALUE: &str = "off";
const ACCEPTED_VALUES: &str = "unset or `off`";

/// The resolved state of the measurement-only optimizer pipeline switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchState {
    /// Run the regular optimizer pipeline.
    On,
    /// Skip the optimizer pipeline and use its existing materialized fallback.
    Off,
}

impl SwitchState {
    /// Returns the stable lowercase representation used by the CLI probe JSON.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
        }
    }
}

/// An invalid value supplied for [`ENVIRONMENT_VARIABLE`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidEnvValue {
    value: String,
}

impl fmt::Display for InvalidEnvValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid {ENVIRONMENT_VARIABLE} value `{}`; accepted values: {ACCEPTED_VALUES}",
            self.value
        )
    }
}

impl std::error::Error for InvalidEnvValue {}

static SWITCH_STATE: OnceLock<Result<SwitchState, InvalidEnvValue>> = OnceLock::new();

/// Resolves the measurement switch once for this process.
///
/// An unset variable resolves to [`SwitchState::On`], and the only accepted
/// set value is exactly `off`, which resolves to [`SwitchState::Off`].
pub fn resolve() -> Result<SwitchState, InvalidEnvValue> {
    SWITCH_STATE.get_or_init(resolve_from_environment).clone()
}

/// Returns whether the normal optimizer pipeline should run.
///
/// The optimizer cannot propagate an environment error through its existing
/// return type. Library parsing therefore panics rather than silently treating
/// an invalid measurement setting as ON; the CLI calls [`resolve`] at startup
/// and reports the same error normally.
pub(crate) fn optimizer_pipeline_enabled() -> bool {
    match resolve() {
        Ok(SwitchState::On) => true,
        Ok(SwitchState::Off) => false,
        Err(error) => panic!("{error}"),
    }
}

fn resolve_from_environment() -> Result<SwitchState, InvalidEnvValue> {
    resolve_value(env::var_os(ENVIRONMENT_VARIABLE))
}

fn resolve_value(value: Option<OsString>) -> Result<SwitchState, InvalidEnvValue> {
    match value {
        None => Ok(SwitchState::On),
        Some(value) if value == OFF_VALUE => Ok(SwitchState::Off),
        Some(value) => Err(InvalidEnvValue {
            value: value.to_string_lossy().into_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_value, SwitchState, ENVIRONMENT_VARIABLE};
    use std::ffi::OsString;

    #[test]
    fn unset_value_enables_the_optimizer_pipeline() {
        assert_eq!(resolve_value(None), Ok(SwitchState::On));
    }

    #[test]
    fn exact_off_value_disables_the_optimizer_pipeline() {
        assert_eq!(
            resolve_value(Some(OsString::from("off"))),
            Ok(SwitchState::Off)
        );
    }

    #[test]
    fn every_other_set_value_is_invalid() {
        let error = resolve_value(Some(OsString::from("OFF")))
            .expect_err("only the exact lowercase off value is accepted");
        let rendered = error.to_string();
        assert!(rendered.contains(ENVIRONMENT_VARIABLE));
        assert!(rendered.contains("OFF"));
        assert!(rendered.contains("unset"));
        assert!(rendered.contains("off"));
    }
}
