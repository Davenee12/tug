fn main() {
    // tauri-build embeds icons/icon.ico into the exe but doesn't watch it, so an
    // icon change alone would otherwise ship the stale icon.
    println!("cargo:rerun-if-changed=icons/icon.ico");
    tauri_build::build()
}
