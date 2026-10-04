//! Compiles `proto/scrin/v1/*.proto` with prost-build and a vendored `protoc`.

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let proto_root = manifest_dir.join("..").join("..").join("proto");
    let v1 = proto_root.join("scrin").join("v1");

    let mut files: Vec<PathBuf> = std::fs::read_dir(&v1)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "proto"))
        .collect();
    files.sort();

    println!("cargo:rerun-if-changed={}", v1.display());
    for f in &files {
        println!("cargo:rerun-if-changed={}", f.display());
    }

    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);
    config.compile_protos(&files, &[proto_root])?;
    Ok(())
}
