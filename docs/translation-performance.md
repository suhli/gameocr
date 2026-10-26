# Translation 性能记录

首轮 CLI 真模型测试使用 Hy-MT2-1.8B Q4_K_M，固定 revision/hash 见 [Runtime](translation-runtime.md)。CPU 4 线程、context 4096、maxTokens 512、temperature 0.7、top_p 0.6、top_k 20、repeat 1.05。GPU 是 NVIDIA GeForce GTX1650 4 GiB；CUDA Worker 在本机 CUDA 12.1 工具链构建。CPU/GPU 各运行 13 条，包含模型加载后首句与复用 context 后的句子，均无翻译缓存。

- CPU 原生模型初始化与预热约 1679 ms；13 条完整生成耗时约 633–1437 ms，TTFT 约 379–799 ms。
- CUDA 原生初始化与预热约 1052 ms；完整生成约 204–385 ms，TTFT 约 118–222 ms。
- CPU 生成阶段约 19–22 token/s；CUDA 约 71–84 token/s。

这些是本机短语料单轮观察，不是平均游戏帧率或性能保证。CLI 的 modelLoadMs 不含文件 SHA 校验；Worker Ready 的 load_ms 包含 Factory 校验及实际加载。GPU 后端准备时间由桌面单独记录，不能计入每次生成时间。只有首 Token 之后的输出阶段用于 tokens/s；TTFT 包含 Prompt evaluation。详见原始 JSON，不应对单轮数据计算显著性。

桌面指标分开记录 OCR 本身、文本处理、排队、Prompt、TTFT、翻译推理、OCR 可用到译文可用、截图到译文，以及进程 Working Set。缓存命中不带推理 token/s，不算神经网络速度。显存读数是 GPU 0 的可用显存，准入估算并不等于实际模型占用。

端到端验证入口为 `scripts/desktop-translation.mjs`，使用真实 Win32 测试窗口、WGC、PP-OCRv6 和 Hy-MT2；报告包含 CPU+CPU、CPU+GPU、GPU+GPU、缓存、防抖、取消、崩溃恢复和 UI 展示。可通过 `GAMEOCR_APP` 指向测试包，通过 `GAMEOCR_GPU_CACHE` 复用已经校验的官方下载缓存；测试必须串行运行（应用有单实例保护）。只有实际生成的 `test-results/translation-desktop.json` 才表示该版本桌面验证完成。

仍需更长时间游戏负载实验、不同分辨率/显卡、真实游戏截图语料和干净 Windows 11 环境。当前测试窗口是明确的合成语料；不会据此声称游戏帧率不受影响。
