//! End-to-end verification of notebook import propagation.
//!
//! Venus compiles a notebook's imports and type definitions into a shared
//! `venus_universe` crate, then compiles each cell as its own crate that links
//! the universe and glob-imports it (`extern crate venus_universe; use
//! venus_universe::*;`). For a notebook `use` statement to be usable by a cell,
//! the universe must re-export it as `pub use` — a private `use` stays internal
//! to the universe crate and the imported name never reaches the cell.
//!
//! These tests reproduce that two-crate structure with plain `rustc` (no
//! cranelift, no external dependencies) and prove both directions:
//! - a `pub use` re-export IS visible to the cell (compiles, loads, runs);
//! - a private `use` is NOT (the cell fails to compile) — the original bug.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn dylib_ext() -> &'static str {
    if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}

/// Compile a source file with stable `rustc`, returning the raw output.
fn rustc(dir: &Path, args: &[&str]) -> Output {
    Command::new("rustc")
        .current_dir(dir)
        .args(["--edition", "2021"])
        .args(args)
        .output()
        .expect("failed to invoke rustc")
}

/// Build the shared universe rlib with the given import section.
///
/// Mirrors the relevant shape of the generated `venus_universe` lib.rs: lint
/// allowances plus the notebook's imports. `import_section` is written verbatim
/// so the caller controls whether the import is `pub use` or a private `use`.
fn build_universe(dir: &Path, import_section: &str) -> Output {
    let lib = format!("#![allow(unused_imports)]\n#![allow(dead_code)]\n{import_section}\n");
    std::fs::write(dir.join("universe.rs"), lib).unwrap();
    rustc(
        dir,
        &[
            "--crate-name",
            "venus_universe",
            "--crate-type",
            "rlib",
            "-o",
            "libvenus_universe.rlib",
            "universe.rs",
        ],
    )
}

/// Build a cell cdylib linked against the universe rlib. The cell body mirrors
/// the real cell wrapper: it links the universe and glob-imports it, then names
/// `BTreeMap` — which is only reachable if the universe re-exported the import.
fn build_cell(dir: &Path) -> Output {
    let cell = r#"
extern crate venus_universe;
use venus_universe::*;

#[no_mangle]
pub extern "C" fn cell_len() -> usize {
    let mut m: BTreeMap<String, i32> = BTreeMap::new();
    m.insert("a".to_string(), 1);
    m.insert("b".to_string(), 2);
    m.len()
}
"#;
    std::fs::write(dir.join("cell.rs"), cell).unwrap();

    let rlib = dir.join("libvenus_universe.rlib");
    let out_name = format!("libcell.{}", dylib_ext());
    rustc(
        dir,
        &[
            "--crate-type",
            "cdylib",
            "--extern",
            &format!("venus_universe={}", rlib.display()),
            "-o",
            &out_name,
            "cell.rs",
        ],
    )
}

fn temp_dir() -> PathBuf {
    let base = std::env::temp_dir().join(format!(
        "venus_import_propagation_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&base).unwrap();
    base
}

#[test]
fn pub_use_reexport_reaches_cell() {
    let dir = temp_dir();

    let uni = build_universe(&dir, "pub use std::collections::BTreeMap;");
    assert!(
        uni.status.success(),
        "universe failed to compile:\n{}",
        String::from_utf8_lossy(&uni.stderr)
    );

    let cell = build_cell(&dir);
    assert!(
        cell.status.success(),
        "cell failed to compile against a `pub use` re-export — the notebook \
         import did not reach the cell:\n{}",
        String::from_utf8_lossy(&cell.stderr)
    );

    // Load the cell and confirm the imported type actually works at runtime.
    let lib_path = dir.join(format!("libcell.{}", dylib_ext()));
    let len = unsafe {
        let lib = libloading::Library::new(&lib_path).expect("failed to load cell dylib");
        let cell_len: libloading::Symbol<extern "C" fn() -> usize> =
            lib.get(b"cell_len").expect("cell_len symbol not found");
        cell_len()
    };
    assert_eq!(len, 2, "expected BTreeMap with 2 entries");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn private_use_does_not_reach_cell() {
    let dir = temp_dir();

    // A private `use` in the universe — the original bug. The universe itself
    // compiles fine, but the imported name is not a public item...
    let uni = build_universe(&dir, "use std::collections::BTreeMap;");
    assert!(
        uni.status.success(),
        "universe with a private import should still compile:\n{}",
        String::from_utf8_lossy(&uni.stderr)
    );

    // ...so the cell that names `BTreeMap` through `use venus_universe::*` must
    // fail to compile — exactly the "not found in this scope" symptom.
    let cell = build_cell(&dir);
    assert!(
        !cell.status.success(),
        "cell unexpectedly compiled against a PRIVATE universe import; the \
         negative control for the import-propagation bug did not reproduce"
    );
    let stderr = String::from_utf8_lossy(&cell.stderr);
    assert!(
        stderr.contains("cannot find")
            || stderr.contains("not found")
            || stderr.contains("E0412")
            || stderr.contains("E0433"),
        "expected a name-resolution error for BTreeMap, got:\n{stderr}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
