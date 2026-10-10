//! Generates the Tauri context. App commands are declared in the manifest so each window's
//! capability can grant only the commands it needs.

const COMMANDS: &[&str] = &[
    "classify_instagram_url",
    "has_token",
    "save_token",
    "clear_token",
    "list_posts",
];

fn main() {
    // The frontend is embedded at compile time; rebuild when any of its files change.
    println!("cargo:rerun-if-changed=../ui");
    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    if let Err(e) = tauri_build::try_build(attributes) {
        eprintln!("tauri-build failed: {e:#}");
        std::process::exit(1);
    }
}
