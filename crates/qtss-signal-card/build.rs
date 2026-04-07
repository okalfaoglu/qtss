//! Ensures `assets/DejaVuSans.ttf` exists so `include_bytes!` works on headless builders.
//! Prefers a committed file; otherwise tries `curl` then `wget` (same URL as `assets/README.txt`).

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const FONT_URL: &str =
    "https://raw.githubusercontent.com/dejavu-fonts/dejavu-fonts/version_2_37/ttf/DejaVuSans.ttf";
const MIN_BYTES: u64 = 10_000;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let font_path = manifest_dir.join("assets/DejaVuSans.ttf");

    let ok = font_path.exists()
        && fs::metadata(&font_path)
            .map(|m| m.len() >= MIN_BYTES)
            .unwrap_or(false);

    if ok {
        println!("cargo:rerun-if-changed={}", font_path.display());
        return;
    }

    eprintln!("qtss-signal-card: embedding font — fetching DejaVuSans.ttf …");
    if let Some(parent) = font_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let curl_ok = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(&font_path)
        .arg(FONT_URL)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !curl_ok {
        let _ = Command::new("wget")
            .args(["-q", "-O"])
            .arg(&font_path)
            .arg(FONT_URL)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    }

    let ok_after = font_path.exists()
        && fs::metadata(&font_path)
            .map(|m| m.len() >= MIN_BYTES)
            .unwrap_or(false);

    if !ok_after {
        panic!(
            "qtss-signal-card: missing or invalid {}.\n\
             Place DejaVuSans.ttf next to this file (see assets/README.txt) or install curl/wget and retry.",
            font_path.display()
        );
    }

    println!("cargo:rerun-if-changed={}", font_path.display());
}
