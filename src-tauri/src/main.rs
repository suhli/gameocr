#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    let mut args = std::env::args_os().skip(1);
    if args.next().is_some_and(|a| a == "--gpu-catalog") {
        let output = args.next().expect("GPU catalog output path");
        std::fs::write(output, gameocr_app::cuda_runtime::backend_catalog())
            .expect("Export embedded GPU catalog");
        return;
    }
    gameocr_app::run();
}
