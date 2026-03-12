use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let deps = [
        manifest_dir.join("wit/io"),
        manifest_dir.join("wit/clocks"),
        manifest_dir.join("wit/random"),
        manifest_dir.join("wit/filesystem"),
        manifest_dir.join("wit/sockets"),
        manifest_dir.join("wit/cli"),
    ];

    let mut deps_literals = String::new();
    for dep in deps {
        deps_literals.push_str(&format!("{:?},", dep.display().to_string()));
    }

    let generated = format!(
        r#"
pub const WASI_WIT_DIRS: &[&str] = &[{deps_literals}];

#[macro_export]
macro_rules! bindgen {{
    ({{ $($body:tt)* }}) => {{
        ::telomere_component_bindgen::bindgen!({{
            $($body)*,
            deps: [{deps_literals}],
            adopt: {{ "wasi:" => ::telomere_component_wasi::bindings }}
        }});
    }};
}}
"#
    );

    fs::write(out_dir.join("generated_bindgen.rs"), generated).unwrap();
}
