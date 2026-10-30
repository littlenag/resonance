//! Embeds the web UI's bundle, `web/dist`, built by `just ui-build` (MVP-043). Without it, a
//! placeholder page says how to build it, so `cargo build` needs no Node.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            files(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let dist = manifest.join("../../web/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    println!("cargo:rerun-if-changed=src/placeholder.html");
    let mut list = Vec::new();
    files(&dist, &mut list);
    let built = list.iter().any(|p| p.ends_with("index.html"));
    let mut code = String::from(
        "/// The embedded bundle: (path, bytes).\npub static ASSETS: &[(&str, &[u8])] = &[\n",
    );
    if built {
        for p in &list {
            let rel = p
                .strip_prefix(&dist)
                .expect("inside dist")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            let abs = fs::canonicalize(p).expect("an asset path");
            let _ = writeln!(
                code,
                "    ({rel:?}, include_bytes!({:?})),",
                abs.display().to_string()
            );
        }
    } else {
        let placeholder =
            fs::canonicalize(manifest.join("src/placeholder.html")).expect("placeholder");
        let _ = writeln!(
            code,
            "    (\"index.html\", include_bytes!({:?})),",
            placeholder.display().to_string()
        );
    }
    code.push_str("];\n");
    let _ = writeln!(
        code,
        "/// Whether the real bundle was embedded.\npub const BUILT: bool = {built};"
    );
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("out dir")).join("assets.rs");
    fs::write(out, code).expect("writing assets.rs");
}
