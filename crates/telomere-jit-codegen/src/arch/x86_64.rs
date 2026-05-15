use crate::target::{TargetArch, TargetInfo, TargetOs};

pub fn target_info() -> TargetInfo {
    TargetInfo {
        arch: TargetArch::X86_64,
        os: target_os(),
        baseline_supported: false,
    }
}

const fn target_os() -> TargetOs {
    #[cfg(target_os = "macos")]
    {
        TargetOs::Macos
    }
    #[cfg(target_os = "linux")]
    {
        TargetOs::Linux
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        TargetOs::Unsupported
    }
}
