use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=asm/probe.S");
    for name in ["MOS_CLANG", "LLVM_MOS", "HOME"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let directory = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let compiler = env::var("MOS_CLANG").unwrap_or_else(|_| {
        let sdk = env::var("LLVM_MOS").unwrap_or_else(|_| {
            format!("{}/.local/llvm-mos", env::var("HOME").unwrap_or_default())
        });
        format!("{sdk}/bin/mos-c64-clang")
    });
    let object = output.join("probe.o");
    let status = Command::new(&compiler)
        .args(["-c", "-o"])
        .arg(&object)
        .arg(directory.join("asm/probe.S"))
        .status()
        .unwrap_or_else(|error| panic!("cannot run {compiler}: {error}"));
    assert!(status.success(), "assembly of probe.S failed");
    println!("cargo:rustc-link-arg={}", object.display());
}
