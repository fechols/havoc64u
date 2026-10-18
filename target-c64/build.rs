use std::{env, path::PathBuf, process::Command};

fn main() {
    for source in [
        "build.rs",
        "link.ld",
        "asm/platform.S",
        "asm/pack.S",
        "asm/memcpy.S",
        "asm/raster.S",
        "asm/division.S",
        "asm/sort.S",
    ] {
        println!("cargo:rerun-if-changed={source}");
    }
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
    for source in ["platform", "pack", "memcpy", "raster", "division", "sort"] {
        let object = output.join(format!("{source}.o"));
        let status = Command::new(&compiler)
            .args(["-c", "-o"])
            .arg(&object)
            .arg(directory.join(format!("asm/{source}.S")))
            .status()
            .unwrap_or_else(|error| panic!("cannot run {compiler}: {error}"));
        assert!(status.success(), "assembly of {source}.S failed");
        println!("cargo:rustc-link-arg={}", object.display());
    }

    // The IRQ wrapper saves the fixed 32 imaginary registers. Extra registers
    // assigned by the SDK's whole-program zero-page allocator would be unsafe.
    println!("cargo:rustc-link-arg=-mlto-zp=0");
    // Pass -T to the clang driver itself. Hiding it inside -Wl would cause
    // clang to append its default link.ld as a second, conflicting script.
    println!("cargo:rustc-link-arg=-T");
    println!(
        "cargo:rustc-link-arg={}",
        directory.join("link.ld").display()
    );
    // Exactly six phase sections deliberately share $3000-$71FF. Linker
    // assertions and tools/check-layout.py enforce every real memory boundary.
    println!("cargo:rustc-link-arg=-Wl,--no-check-sections");
    // The overlay audit uses retained section relocations to distinguish
    // targets that deliberately share the same execution address.
    println!("cargo:rustc-link-arg=-Wl,--emit-relocs");
    println!(
        "cargo:rustc-link-arg=-Wl,-Map,{}",
        output.join("havoc.map").display()
    );
}
