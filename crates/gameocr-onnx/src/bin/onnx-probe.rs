//! Read-only capability probe. Being compiled into ORT does not prove an EP can
//! build this model or run on the installed GPU; no engine cache is created.
use ort::ep::ExecutionProvider;
fn main() -> anyhow::Result<()> {
    gameocr_onnx::initialize_runtime()?;
    println!(
        "{}",
        serde_json::json!({
            "cpuCompiled":ort::ep::CPU::default().is_available()?,
            "cudaCompiled":ort::ep::CUDA::default().is_available()?,
            "tensorRtCompiled":ort::ep::TensorRT::default().is_available()?,
            "tensorRtRtxCompiled":ort::ep::NVRTX::default().is_available()?,
            "modelBuildTested":false
        })
    );
    Ok(())
}
