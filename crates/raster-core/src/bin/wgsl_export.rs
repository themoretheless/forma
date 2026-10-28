//! Exports the WGSL sources and variant goldens to the TypeScript shader
//! library.
//!
//! Usage:
//!   cargo run --manifest-path crates/Cargo.toml -p raster-core --bin wgsl_export
//!   cargo run --manifest-path crates/Cargo.toml -p raster-core --bin wgsl_export -- --check

use raster_core::codegen as codegen;

const GENERATED: [(&str, fn() -> String, fn() -> std::path::PathBuf); 4] = [
    ("sources", codegen::generate_ts_sources, codegen::generated_ts_path),
    ("variant goldens", codegen::generate_variant_goldens_ts, codegen::variant_goldens_path),
    ("TS layouts", codegen::generate_layouts_ts, codegen::layouts_ts_path),
    ("Rust layouts", codegen::generate_layouts_rs, codegen::layouts_rs_path),
];

fn main() {
    let check = std::env::args().any(|arg| arg == "--check");
    let mut failed = false;
    for (label, generate, path) in GENERATED {
        let path = path();
        if check {
            match std::fs::read_to_string(&path) {
                Ok(on_disk) if on_disk == generate() => {
                    println!("wgsl_export: {label}: {} is up to date", path.display());
                }
                Ok(_) => {
                    eprintln!("wgsl_export: {label}: {} is stale — regenerate", path.display());
                    failed = true;
                }
                Err(error) => {
                    eprintln!("wgsl_export: {label}: cannot read {}: {error}", path.display());
                    failed = true;
                }
            }
        } else {
            let content = generate();
            let result = (|| -> std::io::Result<()> {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&path, content)
            })();
            match result {
                Ok(()) => println!("wgsl_export: {label}: wrote {}", path.display()),
                Err(error) => {
                    eprintln!("wgsl_export: {label}: write failed: {error}");
                    failed = true;
                }
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}
