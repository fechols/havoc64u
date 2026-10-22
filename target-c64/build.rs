use std::{env, path::PathBuf, process::Command};

fn main() {
    for source in [
        "build.rs",
        "link.ld",
        "../tools/branch-audit.py",
        "asm/platform.S",
        "asm/pack.S",
        "asm/memcpy.S",
        "asm/raster.S",
        "asm/spans.S",
        "asm/front_spans.S",
        "asm/division.S",
        "asm/sort.S",
        "asm/preparation.S",
        "asm/game_math.S",
        "asm/multiply.S",
        "asm/project.S",
        "asm/emit.S",
        "asm/cells.S",
        "asm/publish.S",
        "asm/hud.S",
        "asm/missile.S",
        "asm/shots.S",
        "asm/flight.S",
    ] {
        println!("cargo:rerun-if-changed={source}");
    }
    for name in ["MOS_CLANG", "LLVM_MOS", "HOME", "HAVOC_SPAN_PREPARED_POINTERS"] {
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
    let span_buffer = env::var_os("CARGO_FEATURE_SPAN_BUFFER").is_some();
    let span_front = env::var_os("CARGO_FEATURE_SPAN_FRONT_TO_BACK").is_some();
    // Deferred-backend ablation only: select at assembly time, with no test
    // in the production row loop. Front always advances its prepared pointers.
    let span_pointers = env::var("HAVOC_SPAN_PREPARED_POINTERS").unwrap_or_else(|_| "1".into());
    assert!(span_pointers == "0" || span_pointers == "1",
            "HAVOC_SPAN_PREPARED_POINTERS must be 0 or 1");
    for source in [
        "platform",
        "pack",
        "memcpy",
        "raster",
        "spans",
        "front_spans",
        "division",
        "sort",
        "preparation",
        "game_math",
        "multiply",
        "project",
        "emit",
        "cells",
        "publish",
        "hud",
        "missile",
        "shots",
        "flight",
    ] {
        if source == "spans" && (!span_buffer || span_front)
            || source == "front_spans" && !span_front
        {
            continue;
        }
        let object = output.join(format!("{source}.o"));
        let mut assembly = Command::new(&compiler);
        if span_buffer {
            assembly.arg("-Wa,-defsym,SPAN_BUFFER=1");
            assembly.arg(format!("-Wa,-defsym,SPAN_PREPARED_POINTERS={span_pointers}"));
        }
        if span_front {
            assembly.arg("-Wa,-defsym,SPAN_FRONT=1");
        }
        let status = assembly.args(["-c", "-o"])
            .arg(&object)
            .arg(directory.join(format!("asm/{source}.S")))
            .status()
            .unwrap_or_else(|error| panic!("cannot run {compiler}: {error}"));
        assert!(status.success(), "assembly of {source}.S failed");
        // The assembler wraps out-of-range conditional branches silently;
        // refuse to link a source containing one (the audit re-assembles it
        // with line information to compare every label with its branch).
        let mut audit_command = Command::new("python3");
        audit_command.arg(directory.join("../tools/branch-audit.py"));
        if span_buffer {
            audit_command.arg("--define").arg("SPAN_BUFFER=1");
            audit_command.arg("--define").arg(format!("SPAN_PREPARED_POINTERS={span_pointers}"));
        }
        if span_front {
            audit_command.arg("--define").arg("SPAN_FRONT=1");
        }
        let audit = audit_command
            .arg(directory.join(format!("asm/{source}.S")))
            .env("MOS_CLANG", &compiler)
            .status()
            .unwrap_or_else(|error| panic!("cannot run the branch audit: {error}"));
        assert!(audit.success(), "branch audit failed for {source}.S");
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
    // Phase sections and four alternative SIM tails share $3000-$71FF. Linker
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
