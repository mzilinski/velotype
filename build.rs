fn main() {
    println!("cargo:rerun-if-changed=resources/windows/velin.rc");
    println!("cargo:rerun-if-changed=assets/icon/velin.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("resources/windows/velin.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("failed to compile Velin Windows resources");
    }
}
