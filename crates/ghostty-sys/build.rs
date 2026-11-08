//! Builds libghostty-vt from the `ghostty` git submodule with Zig, links the
//! static library, and generates Rust bindings for `<ghostty/vt.h>`.
//!
//! Environment variables:
//!
//! - `ZIG`: the Zig executable to use (default: `zig` on `PATH`). The
//!   required Zig version is `minimum_zig_version` in
//!   `ghostty/build.zig.zon`.
//! - `GHOSTTY_SYS_OPTIMIZE`: Zig optimize mode (`Debug`, `ReleaseSafe`,
//!   `ReleaseFast`, `ReleaseSmall`). Defaults to `Debug` for `opt-level=0`,
//!   `ReleaseSmall` for `opt-level=s|z`, and `ReleaseFast` otherwise.
//! - `GHOSTTY_SYS_ZIG_TARGET`: Zig target triple. Defaults to the host when
//!   not cross-compiling, otherwise to a triple derived from the Cargo target.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let source = manifest_dir.join("ghostty");

    if !source.join("build.zig").is_file() {
        panic!(
            "ghostty sources not found at {}; run `git submodule update --init`",
            source.display()
        );
    }

    for path in [
        "build.zig",
        "build.zig.zon",
        "src",
        "include",
        "pkg",
        "vendor",
    ] {
        println!("cargo:rerun-if-changed={}", source.join(path).display());
    }
    for var in ["ZIG", "GHOSTTY_SYS_OPTIMIZE", "GHOSTTY_SYS_ZIG_TARGET"] {
        println!("cargo:rerun-if-env-changed={var}");
    }

    let prefix = out_dir.join("ghostty-vt");
    build_lib(&source, &out_dir, &prefix);
    link_lib(&prefix);
    generate_bindings(&prefix, &out_dir);
}

fn build_lib(source: &Path, out_dir: &Path, prefix: &Path) {
    let zig = env::var_os("ZIG").unwrap_or_else(|| "zig".into());
    let mut cmd = Command::new(&zig);
    cmd.current_dir(source)
        .arg("build")
        .arg("-Demit-lib-vt")
        .arg(format!("-Doptimize={}", optimize_mode()))
        .arg("-Dcpu=baseline")
        .arg("--prefix")
        .arg(prefix)
        .arg("--cache-dir")
        .arg(out_dir.join("zig-cache"));
    if let Some(target) = zig_target() {
        cmd.arg(format!("-Dtarget={target}"));
    }

    let status = cmd.status().unwrap_or_else(|err| {
        panic!(
            "failed to run `{}`: {err}; install Zig or set ZIG to its path",
            Path::new(&zig).display()
        )
    });
    if !status.success() {
        panic!("`{cmd:?}` failed with {status}");
    }
}

fn optimize_mode() -> String {
    if let Ok(mode) = env::var("GHOSTTY_SYS_OPTIMIZE") {
        return mode;
    }
    match env::var("OPT_LEVEL").as_deref() {
        Ok("0") => "Debug",
        Ok("s" | "z") => "ReleaseSmall",
        _ => "ReleaseFast",
    }
    .to_owned()
}

/// The `-Dtarget` value, or `None` to build for the host.
fn zig_target() -> Option<String> {
    if let Ok(target) = env::var("GHOSTTY_SYS_ZIG_TARGET") {
        return Some(target);
    }
    if env::var("TARGET").ok() == env::var("HOST").ok() {
        return None;
    }

    let target = env::var("TARGET").unwrap();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let abi = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let unsupported =
        || -> ! { panic!("no Zig target known for `{target}`; set GHOSTTY_SYS_ZIG_TARGET") };

    let arch = match arch.as_str() {
        "x86" | "x86_64" | "aarch64" | "riscv64" | "wasm32" => arch.as_str(),
        _ => unsupported(),
    };
    let os_abi = match (os.as_str(), abi.as_str()) {
        ("macos", _) => "macos".to_owned(),
        ("ios", _) if target.ends_with("-sim") || arch == "x86_64" => "ios-simulator".to_owned(),
        ("ios", _) => "ios".to_owned(),
        ("linux", "gnu" | "musl") => format!("linux-{abi}"),
        ("android", _) => "linux-android".to_owned(),
        ("freebsd", _) => "freebsd".to_owned(),
        ("windows", "msvc" | "gnu") => format!("windows-{abi}"),
        ("wasi", _) => "wasi".to_owned(),
        ("unknown", _) if arch == "wasm32" => "freestanding".to_owned(),
        _ => unsupported(),
    };
    Some(format!("{arch}-{os_abi}"))
}

fn link_lib(prefix: &Path) {
    let lib_dir = prefix.join("lib");
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-lib=static=ghostty-vt-static");
    } else {
        println!("cargo:rustc-link-lib=static=ghostty-vt");
    }

    // System libraries the static library needs, as recorded by its build.
    let pc = lib_dir.join("pkgconfig/libghostty-vt-static.pc");
    if let Ok(pc) = std::fs::read_to_string(&pc) {
        let private = pc
            .lines()
            .filter_map(|line| line.strip_prefix("Libs.private:"))
            .flat_map(str::split_whitespace);
        for flag in private {
            if let Some(lib) = flag.strip_prefix("-l") {
                println!("cargo:rustc-link-lib={lib}");
            } else if let Some(dir) = flag.strip_prefix("-L") {
                println!("cargo:rustc-link-search=native={dir}");
            } else if let Some(framework) = flag.strip_prefix("-framework") {
                println!("cargo:rustc-link-lib=framework={}", framework.trim());
            }
        }
    }

    // Exposed to dependents as DEP_GHOSTTY_VT_ROOT / DEP_GHOSTTY_VT_INCLUDE.
    println!("cargo:root={}", prefix.display());
    println!("cargo:include={}", prefix.join("include").display());
}

fn generate_bindings(prefix: &Path, out_dir: &Path) {
    let include = prefix.join("include");
    bindgen::Builder::default()
        .header(include.join("ghostty/vt.h").to_string_lossy())
        .clang_arg(format!("-I{}", include.display()))
        .clang_arg("-DGHOSTTY_STATIC")
        // C23 gives every enum an explicit `int` type (GHOSTTY_ENUM_TYPED),
        // matching the library's ABI. Without it clang picks `unsigned int`
        // for enums with no negative values.
        .clang_arg("-std=c23")
        .allowlist_function("ghostty_.*")
        .allowlist_type("Ghostty.*")
        .allowlist_var("GHOSTTY_.*")
        .default_enum_style(bindgen::EnumVariation::Consts)
        .prepend_enum_name(false)
        .derive_default(true)
        .generate_cstr(true)
        .clang_macro_fallback()
        .use_core()
        .merge_extern_blocks(true)
        .generate()
        .expect("failed to generate libghostty-vt bindings")
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("failed to write bindings.rs");
}
