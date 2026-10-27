# Translation 性能记录

2026-10-26 CLI 真模型复测使用 Hy-MT2-1.8B Q4_K_M，固定 revision/hash 见 [Runtime](translation-runtime.md)。Ryzen 5 5600G、4 线程、context 4096、maxTokens 512、temperature 0.7、top_p 0.6、top_k 20、repeat 1.05。GPU 是 NVIDIA GeForce GTX1650 4 GiB，驱动 610.88；CUDA Worker 在本机 CUDA 12.1 工具链构建。CPU/GPU 各运行 14 条，包含加载后首句、复用 context 的句子和长条件句，均无翻译缓存。构建完成后串行测试，未同时运行其他 GameOCR 基准。

- CPU 原生模型初始化与预热约 1265 ms；13 条短文本生成耗时约 717–1366 ms，TTFT 约 384–718 ms；长条件句约 3084 ms。
- CUDA 原生初始化与预热约 1184 ms；短文本生成约 202–382 ms，TTFT 约 119–223 ms；长条件句约 802 ms。
- 短文本输出阶段 CPU 约 16–23 token/s，CUDA 约 72–84 token/s。
- 同条件 CPU 温度 0.2 的短文本约 623–1407 ms，长句约 3115 ms；单轮数据不能证明低温度更快或更准确。

这些是本机短语料单轮观察，不是平均游戏帧率或性能保证。CLI 的 modelLoadMs 不含文件 SHA 校验；Worker Ready 的 load_ms 包含 Factory 校验及实际加载。GPU 后端准备时间由桌面单独记录，不能计入每次生成时间。只有首 Token 之后的输出阶段用于 tokens/s；TTFT 包含 Prompt evaluation。详见原始 JSON，不应对单轮数据计算显著性。

桌面指标分开记录 OCR 本身、文本处理、排队、Prompt、TTFT、翻译推理、OCR 可用到译文可用、截图到译文，以及进程 Working Set。缓存命中不带推理 token/s，不算神经网络速度。显存读数是 GPU 0 的可用显存，准入估算并不等于实际模型占用。

端到端验证入口为 `scripts/desktop-translation.mjs`，使用真实 Win32 测试窗口、WGC、PP-OCRv6 和 Hy-MT2；报告包含 CPU+CPU、CPU+GPU、GPU+GPU、缓存、防抖、取消、崩溃恢复和 UI 展示。可通过 `GAMEOCR_APP` 指向测试包，通过 `GAMEOCR_GPU_CACHE` 复用已经校验的官方下载缓存；测试必须串行运行（应用有单实例保护）。只有实际生成的 `test-results/translation-desktop.json` 才表示该版本桌面验证完成。

2026-10-27 最终便携包完成 16 项桌面检查。正文 ROI 排除测试窗口的标题栏，真实 WGC → PP-OCRv6 CPU → Hy-MT2 CPU 将 “Where are you going?” 译为“你要去哪里？”。该次 OCR 111 ms、翻译推理 581 ms、OCR 可用到译文发布 690 ms、截图到译文发布 830 ms；这些是单次样例，不是 p95。重复帧的 OCR→译文为 0.084 ms，推理 Token 为零且模型调用不增加。三条菜单只调用一次模型，整批推理约 2307 ms；四次逐字更新在 2000 ms 稳定窗口下只调用一次模型。启用持久缓存后重启，首个重复请求仍为缓存命中，模型调用数为零。

同次桌面测试可用显存为 1711 MiB，低于 1024 context 的估算需求加 128 MiB 预留，CPU+GPU/GPU+GPU 组合被准入检查阻止，报告记为 skipped。显式 CUDA 清楚报错；Auto 实际加载 CPU 并生成译文。不能将原生 CLI 的 CUDA 通过替代桌面双 GPU 共存验收，也不能把两种组合声称为已通过。

仍需更长时间游戏负载实验、不同分辨率/显卡、真实游戏截图语料和干净 Windows 11 环境。当前测试窗口是明确的合成语料；不会据此声称游戏帧率不受影响。

`node scripts/test-translation.mjs` 顺序运行默认 CPU、低温度 CPU、CUDA、三条独立菜单合批和单 Token 上限，保存原始译文及测试二进制 SHA。`GAMEOCR_TRANSLATION_MODEL` 指定权重，`GAMEOCR_SKIP_CUDA=1` 可跳过没有硬件支持的 CUDA 测试。CPU 与 CUDA 都验证开头/结尾同句一致；14 条单 Token 测试均达到预算并标记截断，不发布无效 UTF-8。
