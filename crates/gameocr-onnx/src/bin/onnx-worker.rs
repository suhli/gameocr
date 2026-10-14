fn main() -> anyhow::Result<()> {
    let mut factory = gameocr_core::engine::EngineFactory::default();
    factory.register("ppocrv6", gameocr_onnx::PaddleOcrAdapter::create);
    gameocr_core::worker::serve(factory)
}
