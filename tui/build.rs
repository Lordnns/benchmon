use std::env;
use std::path::PathBuf;

fn main() {
    // Compile the C library statically into the Rust binary
    cc::Build::new()
        .file("../src/utils.c")
        .file("../src/setup.c")
        .file("../src/monitor.c")
        .file("../src/verify.c")
        .include("../include")
        .opt_level(2)
        .flag_if_supported("-march=native")
        // Suppress intentional/benign C warnings
        .flag_if_supported("-Wno-builtin-macro-redefined")
        .flag_if_supported("-Wno-stringop-truncation")
        .flag_if_supported("-Wno-format-truncation")
        .compile("benchmon");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    let bindings_out = out_path.join("bindings.rs");

    // Try bindgen if libclang is available; otherwise use pre-generated
    let use_bindgen = env::var("BENCHMON_BINDGEN").is_ok()
        || cfg!(feature = "bindgen");

    if use_bindgen {
        #[cfg(feature = "bindgen")]
        {
            let bindings = bindgen::Builder::default()
                .header("../include/benchmon.h")
                .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
                .derive_debug(true)
                .derive_default(true)
                .allowlist_function("benchmon_.*")
                .allowlist_type("benchmon_.*")
                .allowlist_var("BENCHMON_.*")
                .generate()
                .expect("Unable to generate bindings");
            bindings
                .write_to_file(&bindings_out)
                .expect("Couldn't write bindings!");
        }
    }

    if !bindings_out.exists() {
        // Copy pre-generated bindings as fallback
        std::fs::copy("src/bindings_pregenerated.rs", &bindings_out)
            .expect("Failed to copy pre-generated bindings — \
                     install libclang-dev or ensure src/bindings_pregenerated.rs exists");
    }

    println!("cargo:rerun-if-changed=../include/benchmon.h");
    println!("cargo:rerun-if-changed=../src/");
}