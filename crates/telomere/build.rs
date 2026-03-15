use std::{env, process::Command};

fn main() {
    println!("cargo:rustc-check-cfg=cfg(telomere_nightly)");
    println!("cargo:rerun-if-env-changed=RUSTC");
    println!("cargo:rerun-if-env-changed=RUSTC_WRAPPER");
    println!("cargo:rerun-if-env-changed=RUSTC_WORKSPACE_WRAPPER");
    println!("cargo:rerun-if-env-changed=RUSTUP_TOOLCHAIN");

    let Some(rustc) = env::var_os("RUSTC") else {
        return;
    };
    let Ok(output) = Command::new(rustc).arg("-vV").output() else {
        return;
    };
    if !output.status.success() {
        return;
    }
    let Ok(stdout) = String::from_utf8(output.stdout) else {
        return;
    };

    let is_nightly = stdout.lines().any(|line| {
        line.strip_prefix("release: ")
            .is_some_and(|release| release.contains("nightly"))
    });
    if is_nightly {
        println!("cargo:rustc-cfg=telomere_nightly");
    }
}
