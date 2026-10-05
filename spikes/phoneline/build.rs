// Embeds tug-call-spike.exe.manifest (with the <msix> identity element) into the exe.
fn main() {
    println!("cargo:rerun-if-changed=tug-call-spike.exe.manifest");
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest::embed_manifest_file("tug-call-spike.exe.manifest")
            .expect("unable to embed tug-call-spike.exe.manifest");
    }
}
