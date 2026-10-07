use std::path::{Path, PathBuf};

fn main() {
    // tauri-build embeds icons/icon.ico into the exe but doesn't watch it, so an
    // icon change alone would otherwise ship the stale icon.
    println!("cargo:rerun-if-changed=icons/icon.ico");
    embed_tugboat_page();
    ensure_cli();
    tauri_build::build()
}

/// The `tug` command (`crates/tug-cli`, built by `npm run build:cli`) ships inside tug's install
/// folder as `bin\tug.exe` (tauri.conf.json `bundle.resources`), and tauri-build insists the file
/// exists. A debug build or a bare `cargo test` gets an empty stand-in so checks don't need the
/// command built first (Settings hides the command while it's empty); a release build must have
/// the real one, so a forgotten `build:cli` fails loudly instead of shipping a dud.
fn ensure_cli() {
    let cli = Path::new("binaries").join("tug-cli.exe");
    println!("cargo:rerun-if-changed={}", cli.display());
    let real = std::fs::read(&cli).is_ok_and(|b| b.starts_with(b"MZ"));
    if real {
        return;
    }
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        panic!("binaries/tug-cli.exe is missing: run `npm run build:cli -- --release` before a release build");
    }
    if !cli.exists() {
        std::fs::create_dir_all("binaries").expect("create binaries/");
        std::fs::write(&cli, b"").expect("write the tug-cli placeholder");
    }
}

/// Embed the Tugboat phone page (built by `npm run build` into `tugboat-page/dist`) as a table of
/// `(url path, bytes)` for `src/tugboat/page.rs`. The `tugboat-page` folder is committed (only its
/// `dist` is ignored), so cargo can watch it and re-embed whenever the page is rebuilt. With no
/// build yet (a bare `cargo test`), a placeholder page stands in.
fn embed_tugboat_page() {
    println!("cargo:rerun-if-changed=tugboat-page");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let dist = Path::new("tugboat-page").join("dist");
    let mut files = Vec::new();
    if dist.join("index.html").is_file() {
        collect(&dist, &dist, &mut files);
    }
    files.sort();
    let mut table = String::from("static ASSETS: &[(&str, &[u8])] = &[\n");
    if files.is_empty() {
        let placeholder = out.join("tugboat-placeholder.html");
        std::fs::write(
            &placeholder,
            "<!doctype html><meta charset=utf-8><title>Tugboat · tug</title>\
             <p>The Tugboat page wasn't built. Run <code>npm run build</code>.</p>",
        )
        .expect("write the placeholder page");
        files.push(("/index.html".to_string(), placeholder));
    }
    for (url, path) in &files {
        let abs = std::fs::canonicalize(path).expect("canonicalize a Tugboat page file");
        table.push_str(&format!(
            "    ({url:?}, include_bytes!({:?})),\n",
            abs.display().to_string()
        ));
    }
    table.push_str("];\n");
    std::fs::write(out.join("tugboat_page.rs"), table).expect("write tugboat_page.rs");
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out);
        } else {
            let rel = path.strip_prefix(root).expect("inside the dist folder");
            let url = format!("/{}", rel.to_string_lossy().replace('\\', "/"));
            out.push((url, path));
        }
    }
}
