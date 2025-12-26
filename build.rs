use std::env;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let ufbx_dir = env::var("UFBX_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join("vendor/ufbx"));
    let ufbx_c = ufbx_dir.join("ufbx.c");
    let ufbx_h = ufbx_dir.join("ufbx.h");

    if !ufbx_c.exists() || !ufbx_h.exists() {
        panic!("ufbx source not found. Set UFBX_DIR or place ufbx.c/ufbx.h under vendor/ufbx.");
    }

    let mut build = cc::Build::new();
    build.file(&ufbx_c);
    build.file(manifest_dir.join("src/ufbx_wrapper.c"));
    build.include(&ufbx_dir);
    build.include(manifest_dir.join("src"));
    build.flag_if_supported("-std=c99");
    build.compile("ufbx");

    println!("cargo:rerun-if-env-changed=UFBX_DIR");
    println!("cargo:rerun-if-changed={}", ufbx_c.display());
    println!("cargo:rerun-if-changed={}", ufbx_h.display());
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("src/ufbx_wrapper.c").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("src/ufbx_wrapper.h").display()
    );
}
