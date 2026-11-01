//! Packs the release hilal.exe into the installer, and embeds the installer's icon,
//! version info and manifest. Build hilal first (see dist.ps1).

use std::path::PathBuf;

fn main() {
    let exe = std::env::var("HILAL_EXE").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("../target/release/hilal.exe"));
    println!("cargo:rerun-if-env-changed=HILAL_EXE");
    println!("cargo:rerun-if-changed={}", exe.display());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("payload.exe");
    std::fs::copy(&exe, &out).unwrap_or_else(|e| panic!("can't read {} ({e}); build hilal --release first", exe.display()));

    println!("cargo:rerun-if-changed=setup.rc");
    println!("cargo:rerun-if-changed=setup.manifest");
    println!("cargo:rerun-if-changed=../assets/hilal.ico");
    let _ = embed_resource::compile("setup.rc", embed_resource::NONE);
}
