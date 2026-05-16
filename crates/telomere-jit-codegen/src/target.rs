#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetArch {
    Aarch64,
    Riscv64,
    X86_64,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOs {
    Macos,
    Linux,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetInfo {
    pub arch: TargetArch,
    pub os: TargetOs,
    pub baseline_supported: bool,
}

pub fn current() -> TargetInfo {
    TargetInfo {
        arch: current_arch(),
        os: current_os(),
        baseline_supported: supported(),
    }
}

pub fn supported() -> bool {
    cfg!(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(
            any(target_os = "macos", target_os = "linux"),
            target_arch = "x86_64"
        ),
        all(
            target_os = "linux",
            target_arch = "riscv64",
            target_env = "gnu"
        )
    ))
}

const fn current_arch() -> TargetArch {
    #[cfg(target_arch = "aarch64")]
    {
        TargetArch::Aarch64
    }
    #[cfg(target_arch = "riscv64")]
    {
        TargetArch::Riscv64
    }
    #[cfg(target_arch = "x86_64")]
    {
        TargetArch::X86_64
    }
    #[cfg(not(any(
        target_arch = "aarch64",
        target_arch = "riscv64",
        target_arch = "x86_64"
    )))]
    {
        TargetArch::Unsupported
    }
}

const fn current_os() -> TargetOs {
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
