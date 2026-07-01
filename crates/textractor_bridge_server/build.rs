use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR should be set by Cargo"),
    );
    let web_dist = manifest_dir
        .join("..")
        .join("..")
        .join("web_ui")
        .join("dist");
    let index_html = web_dist.join("index.html");

    println!("cargo:rerun-if-changed={}", web_dist.display());

    if !index_html.is_file() {
        panic!(
            "built web UI not found at {}; run `cd web_ui && npm run build` before building textractor_bridge_server",
            web_dist.display()
        );
    }
}
