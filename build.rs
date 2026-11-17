// Puts the Faber icon into the Windows executable.
fn main() {
    println!("cargo:rerun-if-changed=assets/faber.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/faber.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("compile the Windows icon resource");
    }
}
