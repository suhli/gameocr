# OCR 效率优化：按顺序交付

1. 小字与局部高分辨率检测：CPU / CUDA 原生质量及桌面配置验证通过。
2. 区域级增量识别与缓存：CPU / CUDA 连续帧一致性、来源切换、取消和真实窗口数字更新验证通过。
3. 识别宽度分桶与像素预算：CPU / CUDA 质量通过；CPU 样本提速，GTX1650 样本变慢，默认关闭。
4. 预处理、传输与 CTC 解码开销：原生输出一致性与真实桌面 PPM 传输验证通过。
5. 困难区域选择性复验：CPU / CUDA 及桌面验证通过；CPU 4K 补检 8 个区域完整召回，耗时低于完整分块。
6. GPU FP16 / CPU INT8 的兼容性与质量评估：完成，候选可用但本机未提速，保留 FP32 默认。

所有功能保留兼容旧设置的默认值。基准区分首次识别与连续识别，分别记录耗时和准确率；不把增加召回的额外计算描述为加速。

## 小字检测

`onnx.preserveSmallText` 默认关闭。开启后先做区域概览检测，再做有重叠的原分辨率局部检测，合并框后从完整原图裁字。检测最长边至少 256 px，局部块最多 64 个；超预算明确报错，用户可缩小 ROI 或提高最长边。

`onnx.detectionRegion` 为可选的 `[left, top, width, height]` 比例，范围 0–1。只识别区域内内容，但输出尺寸与坐标仍相对完整截图。

局部、小字、选择性补检和增量模式使用单 Worker 和完整 CPU 线程预算，避免外层分块重复解释 ROI、缓存或先识别断句。手动并行设置保留，加载提示说明实际执行方式；自适应在此配置下只选择 CPU×1。关闭后恢复原策略。

分块会增加检测次数，适合小字召回不足的高分辨率画面。ROI 可以减少检测面积，连续帧可以配合下述增量识别。合并保留倾斜框，对重叠的水平截断行拼接；任意排版仍需实际样本检验。

验证：`scripts/create-small-text-fixtures.ps1` 生成 2K / 4K 合成 UI，各有 53 行、16 px 小字与跨块长句；`node scripts/test-small-text.mjs` 比较整图、小字、ROI、ROI 小字四种配置，结果写入 `docs/test-results/small-text.json`。这不是实际游戏准确率基准。

2026-10-20 CPU Small 实测（每种配置/场景预热 1 次、统计 3 次）：2K 整图与小字均 53/53；4K 整图为 1/53，小字为 53/53。小字模式两种尺寸均 CER=0，无多余块，跨块长句完整。4K 小字中位耗时 5825 ms（19 次检测），整图 323 ms 但大量漏字；右下 ROI 4/4 行完整，中位 203 ms。两者处理范围与召回不同，不能据此宣称同质量加速。修复的回归包括概览粗框重复、轻微倾斜接缝断句和不同 padding 的局部重复框。

## 增量识别

`onnx.incremental` 默认关闭，使用单 Worker。对上一帧检测区域逐行比较 RGB 字节，完全相等才复用检测框；框相同且文字框外扩 16 px 的像素完全相等，才复用文字和原始置信度。新增、删除、滚动或背景变化都会使相应缓存失效，不使用感知哈希或高置信度推断相等。

缓存仅驻留在 Worker 内存，最多保留一帧（RGB 不超过 64 MiB）、4096 行和 12000 个检测候选框，不落盘。每 10 秒或 30 次复用后刷新，尺寸、检测分辨率、模型、来源 generation、取消 epoch 或失败修订变化时失效。未带捕获上下文的独立 Worker 调用不使用缓存。

增量模式替代桌面的缩略图变化门控，否则小数字变化可能在到达 OCR 之前就被跳过。结果的 frameId / timestamp 始终属于当前帧。状态中显示复用区域和文字行数。

CPU 4K 连续序列首次验证：重复帧 31.9 ms，检测 19/19、文字 53/53 全部复用；单数字变化 391.8 ms，复用 17 个检测区域和 52 行；删除文字 363.6 ms；滚动 4073.9 ms，文字全部重新识别，仅复用仍完全相同的空白块。与关闭缓存的识别文本和坐标匹配，无旧文字残留；来源切换和取消后刷新通过。这些是单次序列观测，不是长期 P95 或真实游戏速度保证。报告：`docs/test-results/incremental.json`。

## 宽度分桶

`onnx.widthBuckets` 默认关闭。开启后按 64 px 宽度分桶（320–3200），每批同时受 Batch Size 与 `recognitionPixelBudget` 限制，默认预算 196608 像素（不含 3 个颜色通道）。超长行不会把短行的 padding 拉大；保留原始阅读顺序。缓存键还包括本行所在批次的 padding 宽度，批次变化时重识别，避免相同裁图在不同输入形状下复用旧输出。

CPU 2K 53 行样本，预热 1 次、测量 5 次：CER=0、召回 100%、无额外框；输入像素 1016304→912384，批次 9→11，识别中位耗时 1066→942 ms，原生总耗时 1441→1322 ms。`docs/test-results/batching-cpu.json`。这不代表所有文字长度分布或 GPU 都有同样收益。

GTX1650 对应 2K 样本同样完整召回，但原生总耗时 2389→2687 ms，识别阶段 1956→2287 ms。输入像素减少没有抵消更多小批次的开销，因此不能当作 GPU 通用加速；保留可选开关。[batching-cuda.json](test-results/batching-cuda.json)。

## 预处理、传输与 CTC

ONNX 输入缓冲区在 Worker 中复用，使用 ORT 借用 TensorRef；每批 padding 明确清零。每次只持有一个原始透视裁图，避免同时保存整批大裁图。CTC 将合法性检查与 argmax 合并为一次扫描，保留相同最大值时选择最后一个类别的规则，非最大类别中的 NaN / 越界概率仍会报错。ONNX 单 Worker 与多 Worker 均使用应用管理的临时 PPM，跳过 PNG 编码；GGUF 保留原路径。

优化前后交替宽图、窄图和空白图的原生输出一致，置信度差小于 1e-5，无 padding 残留。密集 2K 样本解析中位 124.7→105.5 ms、预处理 76.9→74.0 ms；总耗时 1304→1274 ms。小样本和空白帧存在波动，不据此宣称普遍加速。`docs/test-results/pipeline.json`。

## 选择性补检

`onnx.selectiveDetection` 默认关闭。概览检测后，在原图的 32 px 单元上统计局部边缘，排除平坦背景、过密噪声和已有清晰框覆盖的单元，按未解释细节为局部区域排序；最多补检 `refinementRegions` 个区域（默认 4，范围 1–8）。所有框在识别前统一合并。完整小字模式优先，不再重复执行选择性补检。

这是有预算的漏检风险启发式，不是文字定位真值，也不以识别置信度证明完整性。纹理、低对比度和极细笔画仍可能误判；预算不足时应缩小 ROI、提高补检预算或启用完整小字模式。

CPU Small，预热 1 次、测量 3 次：4K 整图 1/53、327 ms；补检 4 个区域 49/53、2238 ms；补检 8 个区域 53/53、3247 ms；完整分块 53/53、5616 ms。2K 原整图已经完整召回，补检会增加耗时。补检 8 区与完整分块两尺寸均 CER=0、无额外框，空白帧均为空。报告：[refinement.json](test-results/refinement.json)。

## FP16／INT8 实验

工具仅生成独立模型目录，不覆盖原权重、不修改用户配置、不添加桌面 Python 依赖、不加入默认下载目录。转换校验源文件 SHA-256，保持 FP32 输入／输出和原字典顺序，再写新 Manifest、哈希与转换元数据。

- FP16：使用锁定版本 ONNX Runtime 的 float16 转换器；Softmax、Sigmoid、ReduceMean、Pow 等保留 FP32，转换后重新排列图拓扑并校验。
- INT8：MinMax 静态校准、逐通道权重量化、带符号 INT8 QDQ，仅处理 Conv／MatMul／Gemm。旧 opset 先升到 14 以支持 QDQ axis；校准使用独立的 8 张合成图及 40 个真值框裁图，不使用测试集。
- 校准的 RGB→BGR、归一化、48 px 识别高度与 padding 沿用原生约定；Pillow 插值和真值框裁图不是原生透视预处理的逐像素副本，因此评估必须再次通过真正的 Rust Worker。

```powershell
python -m venv .cache/onnx-tools
.cache/onnx-tools/Scripts/python.exe -m pip install -r scripts/onnx-tools-requirements.txt
.cache/onnx-tools/Scripts/python.exe scripts/optimize-onnx.py --manifest .cache/test-models/ppocrv6-small.json --mode fp16 --output .cache/optimized-models/small-fp16
.cache/onnx-tools/Scripts/python.exe scripts/optimize-onnx.py --manifest .cache/test-models/ppocrv6-small.json --mode int8 --output .cache/optimized-models/small-int8
node scripts/benchmark-precision.mjs
```

输出目录必须不存在；重新实验时选择新的父目录（其中仍命名为 `small-fp16` / `small-int8`），用 `GAMEOCR_OPTIMIZED_MODELS` 指向它。`GAMEOCR_PRECISION_VARIANT` 可选择逗号分隔的 `fp32-cpu,int8-cpu,fp32-cuda,fp16-cuda`，`GAMEOCR_PRECISION_REPORT` 指定报告路径。GPU 测试需要 CUDA Worker；不自动改用 CPU 来伪造 GPU 结果。

2026-10-20，Ryzen 5 5600G / GTX1650 4 GB，驱动 610.88，原生 Small，每场景预热 1 次、测量 3 次。评估图片哈希与校准集无交集；英文、中日繁体关键词通过，空白无输出，2K／4K 的三次测量均 53/53、CER=0、无额外块。框位置与置信度并非逐位一致；INT8 2K 存在与 FP32 的框匹配差异，不将文本全对等同于坐标无变化。报告：[precision.json](test-results/precision.json)。

- CPU：2K FP32 1169 ms、INT8 1347 ms；4K 补检 8 区 FP32 3208 ms、INT8 4323 ms。
- CUDA：2K FP32 2895 ms、混合 FP16 3270 ms；4K 补检 8 区 FP32 5377 ms、混合 FP16 5772 ms。
- 模型加字典由 FP32 31.11 MB 降为 FP16 15.69 MB、INT8 8.62 MB，但测量的五种场景均未提速，**不提升为默认模型**。未测低精度 Medium、其他显卡、真实游戏语料和长期显存峰值；磁盘变小不能当作运行时内存收益。

不改用 GPU ArgMax／设备输出绑定：当前已验证的是 CPU 借用输入缓冲区，仍保留完整概率输出和原 CTC 校验；没有实现或宣称 GPU 零拷贝。

## 最终集成验证

2026-10-20–21 完成 CPU / CUDA 的小字、ROI、宽度分桶、数字修改、删除、滚动、来源切换及取消回归。CUDA 完整小字在 2K / 4K 均 53/53，ROI 均 4/4；增量结果与关闭缓存一致。报告：[CUDA 小字与区域](test-results/efficiency-small-text-cuda.json)、[CPU 增量＋分桶](test-results/efficiency-incremental-cpu.json)、[CUDA 增量＋分桶](test-results/efficiency-incremental-cuda.json)。

正式桌面构建使用内嵌前端。真实 WGC 测试验证设置默认值、持久化和居中、单 Worker 与补检预算、重复帧的新 frameId 和零推理复用；连续 OCR 在缩略图变化阈值设为最大时仍能读到 `Health: 100 → 101`。模型卸载及推理中关闭会清理自有进程和临时文件。分发与限制见 [测试记录](testing.md)。
