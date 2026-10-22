fn main() {
    // A release-specific GPU worker must match this application. Its trusted
    // URL/hash is compiled into the EXE, never fetched from a mutable remote catalog.
    println!("cargo:rerun-if-env-changed=GAMEOCR_GPU_MANIFEST");
    let manifest = std::env::var_os("GAMEOCR_GPU_MANIFEST").map(std::path::PathBuf::from);
    let contents = manifest.map_or_else(
        || "{\"packages\":[]}".to_owned(),
        |path| {
            println!("cargo:rerun-if-changed={}", path.display());
            std::fs::read_to_string(path).expect("Read generated GPU manifest")
        },
    );
    std::fs::write(
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("gpu-backends.json"),
        contents,
    )
    .expect("Embed GPU manifest");
    tauri_build::build()
}
