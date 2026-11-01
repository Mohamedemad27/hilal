fn main() {
    // Icon and version info for hilal.exe.
    println!("cargo:rerun-if-changed=hilal.rc");
    println!("cargo:rerun-if-changed=assets/hilal.ico");
    let _ = embed_resource::compile("hilal.rc", embed_resource::NONE);
}
