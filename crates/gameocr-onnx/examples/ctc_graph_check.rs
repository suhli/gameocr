//! Device-level boundary checks for the exported compact subgraph, not OCR timing.
use anyhow::{Result, ensure};
use ort::{ep, session::Session, value::Tensor};

fn main() -> Result<()> {
    gameocr_onnx::initialize_runtime()?;
    let graph = std::env::args()
        .nth(1)
        .expect("compact validation graph path");
    let report = std::env::args().nth(2).expect("report path");
    let mut session = Session::builder()?
        .with_intra_threads(1)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .with_execution_providers([ep::CUDA::default().build().error_on_failure()])
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .with_profiling(".cache/ctc-graph-cuda")
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .commit_from_file(graph)?;
    let mut checks = Vec::new();
    for (name, values, valid, expected) in [
        ("tie-last", [0., 0.1, 0.8, 0.8], true, Some(3)),
        ("blank", [0.9, 0.1, 0., 0.], true, Some(0)),
        ("nonmax-nan", [f32::NAN, 0.9, 0.1, 0.], false, None),
        ("nonmax-negative", [-0.01, 0.9, 0.1, 0.], false, None),
        ("infinity", [f32::INFINITY, 0.9, 0.1, 0.], false, None),
        ("over-one", [0., 1.1, 0.1, 0.], false, None),
        ("signed-zero", [0., -0., 0., -0.], true, None),
        ("last-class", [0., 0., 0., 0.], true, Some(18709)),
    ] {
        let mut input = values.to_vec();
        input.resize(18710, 0.);
        if name == "last-class" {
            input[18709] = 0.9;
        }
        let output = session.run(ort::inputs![Tensor::from_array((
            [1usize, 1, 18710],
            input
        ))?])?;
        let (_, ids) = output["gameocr_ctc_ids"].try_extract_tensor::<i64>()?;
        let (_, scores) = output["gameocr_ctc_scores"].try_extract_tensor::<f32>()?;
        let (_, flags) = output["gameocr_ctc_valid"].try_extract_tensor::<f32>()?;
        ensure!((flags[0] == 1.) == valid, "{name}: probability validation");
        if let Some(id) = expected {
            ensure!(ids[0] == id, "{name}: expected {id}, got {}", ids[0]);
        }
        if name == "signed-zero" {
            ensure!(scores[0] == 0., "zero maximum must trigger Rust fallback");
        }
        checks.push(name);
    }
    let profile = session.end_profiling()?;
    let events: serde_json::Value = serde_json::from_slice(&std::fs::read(&profile)?)?;
    let argmax_cuda = events.as_array().unwrap().iter().any(|event| {
        event["args"]["op_name"] == "ArgMax" && event["args"]["provider"] == "CUDAExecutionProvider"
    });
    ensure!(argmax_cuda, "ArgMax fell back to CPU");
    std::fs::write(
        report,
        serde_json::to_vec_pretty(&serde_json::json!({
            "scope": "Synthetic boundary tensors through the actual exported compact subgraph on CUDA; not OCR inference measurements",
            "checks": checks, "argmaxCuda": argmax_cuda, "profile": profile
        }))?,
    )?;
    println!("Passed {} CUDA compact graph boundary checks", checks.len());
    Ok(())
}
