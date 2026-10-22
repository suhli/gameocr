# 实时 OCR 延迟优化

本轮在 `feature/multi-ocr-runtime` 上增量开发，沿用 Runtime Factory、独立 Worker、Executor 和 Adaptive。所有新策略默认关闭。数据来自 Ryzen 5 5600G、GTX 1650 4 GB、驱动 610.88、仓库附带 ORT 1.24.3 的实机测量；不代表 RTX 5060 Ti，也不代表真实游戏准确率。

## 1. 基线与计时口径

`scripts/benchmark-latency.mjs` 在禁用缓存时比较原生 Worker、Single、Manual、Adaptive。`docs/test-results/latency-baseline.json` 保留加载、首帧、三次热运行、输出准确率和硬件资源快照。2K、53 行合成小字场景：CPU Single 中位 1229 ms，Manual 2506 ms，Adaptive 1503 ms；CUDA Single 2333 ms，Manual 6050 ms。所有输出 53/53，CER 0。Adaptive 当前只支持 CPU，CUDA Adaptive 明确不支持，没有用 CPU 数字冒充。

原生 CPU 样本中检测约 125 ms、识别约 839 ms、CTC 约 104 ms；CUDA 样本分别约 341、2179、103 ms。识别器输出约 198 MB 的完整概率数组。优先减少重复推理，并评估概率传输，收益不能由 Worker 数量推断。

新增 `metrics.pipeline`：图像读取/解码、检测预处理、DB 后处理（含补检区域规划）、透视裁剪、识别预处理、CTC、框合并、缓存提交、实际检测/识别调用数、识别形状及输出字节数。已有 `detectionMs` / `recognitionMs` 为 ORT Run 墙钟，含同步与隐式拷贝，不能称为纯 GPU kernel 时间。并行阶段数值是工作量之和，可能大于端到端墙钟；整图复验也计入被丢弃切片的工作量。

`execution` 保留传输、排队、Worker 请求墙钟、合并和端到端时间，增加传输方式与 `ipcOverheadMs`（请求墙钟减原生总耗时的估算，含序列化、进程调度）。`appLatency` 记录 WGC 回调后的读回、RGBA→RGB、帧等待、变化门控、预览 JPEG、事件发布。`captureToPublishMs` 从 WGC 回调读回开始到提交发布前；不包括 GPU/OS 在回调前的采集等待，也不包括 WebView 绘制。发布完成后的耗时通过运行统计更新，不能写回已发出的旧事件。时序开启时，预览只在来源、尺寸与全帧 RGB 完全相同且匹配已发布历史时复用 JPEG，并更新为当前 frameId；否则重新编码。ADB 没有 WGC 阶段时为 null。

原生基准不包含截图、宿主传输、JPEG 或界面；Executor 基准包含传输但不包含 WGC/JPEG。采集频率、OCR 间隔与模型延迟共同决定文字变化响应，不能用静态缓存中位数代表变字延迟。默认 OCR 间隔仍为 1000 ms；可设最低 10 ms，时序模式且间隔低于 100 ms 时调度轮询为 5 ms，仍只处理最新帧、最多一个在途任务。

### ORT 检查

两个 Session 随模型加载创建并跨帧复用；输入用借用 TensorRef 和可复用 CPU 缓冲。配置明确设置 intra-op 线程数；inter-op、执行模式、spinning 保留随附 ORT 的默认值，未假设不同版本默认完全相同，也未为测量修改它们。CPU 并行仍分摊总线程预算。动态宽度/批大小会改变形状；现有 64 像素宽度分桶和像素预算可用于 Rec-only，但不同设备的效果必须分开测量。

CUDA 保留已验证的卷积算法 Default、禁止最大卷积 workspace；没有更改这些兼容性设置。两张图加载后均做实际推理，并校验 profiling 中确实存在 CUDA kernel。设置 `GAMEOCR_ORT_PROFILE_DIR` 可保存这次**加载预热**的节点/provider 记录；它不代表长序列 steady-state profile。

初次真实 WGC 测量还暴露了 JPEG 成本：缓存原生约 9 ms，但预览编码 54–78 ms，采集回调至发布约 103–170 ms（`test-results/temporal-wgc-before-preview.json`）。因此新增严格全帧等价的预览复用，单像素或 generation 变化都会拒绝复用；最终桌面复测与原生结果分开报告。

预览复用后的 [真实桌面复测](test-results/temporal-wgc.json) 保留实际配置：1866×1013、CPU Small、Manual 实际限制为单 Worker、共享内存、时序刷新 10 秒，其他补检/分桶/旧增量开关关闭。两帧缓存结果的原生耗时为 5.18/3.92 ms，预览比较与复用为 0.64/0.84 ms；采集回调至发布为 78.98/73.32 ms，其中帧等待为 54.46/54.75 ms。复用降低了重复编码成本，但这组手动触发样本没有实现应用整体低于 50 ms，也不是自动 OCR 延迟的分布。刷新 10 秒只用于桌面生命周期复测，产品默认仍为 1 秒。

最终桌面复测修正了测试源本身：透明背景上反复绘制抗锯齿文字会使字缘逐帧变深，直接清底又可能被 WGC 捕获到重绘中途；测试窗口现在先在内存中清底并绘制，再整帧显示。生产缓存仍要求对应区域像素严格一致，没有放宽校验。`desktop-temporal-debug.json` 仅保留排查过程，其截图写盘改变送帧间隔，不用作最终性能结论。

## 2–3. 时序跟踪与 Rec-only

详见 [temporal-ocr.md](temporal-ocr.md)。跟踪状态扩展现有 FrameCache；独立 `recognizer::recognize_regions` 只持有识别 Session，无检测器入口。Full 与 Rec-only 共用裁剪、动态批处理、宽度分桶、CTC 和坐标逻辑。旧精确像素缓存开关保持可用；开启时序后，由时序的刷新和信任规则控制整个缓存。

最终 Small 对照为 CPU/CUDA 各 18 场景×5 策略、520 帧：均无相对 Full 的文字/位置差异，GT CER 0、逐行准确召回 100%。静态三行 Auto 原生中位为 CPU 2.66 ms / CUDA 2.54 ms；单数字变化帧为 27.42 / 223.62 ms，原生 Worker 请求响应为 28.24 / 224.34 ms。新增区域及时退回 Full，请求响应为 270.56 / 463.04 ms。变字首次响应均无漏掉新字、残留旧字，不代表真实捕获条件下零延迟。2K/4K 静态 Auto 为 CPU 19.78/37.02 ms、CUDA 20.80/41.90 ms，未达到 5 ms；复杂纹理、阴影和渐隐的保护通常回到 Full。

宽度桶专项使用三行动态背景，CPU/CUDA 各批大小 1 与 6，共 120 帧，全部与 Full 一致。强制 Rec-only 中位：CPU 分桶 batch1/batch6 为 57.82/57.84 ms，无桶 batch6 为 69.79 ms；CUDA 对应 648.79/207.02 ms，无桶为 195.17 ms。小批次在本机 CUDA 上明显更慢，分桶也没有普遍收益。Medium HUD 另测 CPU/CUDA 各 30 帧，变化帧 Auto 为 53.45/668.84 ms，均超出 50 ms，不能将 Small 的结论套用到 Medium。专项报告为 `temporal-buckets-*.json`、`temporal-medium-*.json`。

## 4. 实验性 CUDA CTC 压缩

`scripts/create-compact-ctc.py` 离线生成独立模型目录，验证源文件 SHA-256，保留原始 recognizer，另加 `recognizer-ctc` 角色。生成清单标记为本地导入，桌面目录扫描可发现它；原模型和默认目录均不覆盖。GPU 图输出逐时间步的最后一个最大类 ID、最大概率和**全类别有效性**归约；检查所有概率的范围/NaN/Infinity。Rust 沿用 blank、重复字符与平均置信度语义；零最大值回退原始图，保留 signed-zero 的排序边界。类越界、维度/字段错误、非有限数或运行错误都回退，并禁用本 Worker 的实验分支。

实测发现直接使用 `ArgMax(select_last_index=1)` 会回退 CPU；[ORT 1.24.3 的 CUDA 源码](https://github.com/microsoft/onnxruntime/blob/v1.24.3/onnxruntime/core/providers/cuda/cuda_execution_provider.cc) 明确排除了该属性。初版报告保留在 `compact-ctc-cpu-argmax.json` 和 `ctc-provider-profile-cpu-argmax.json`，不能把其声明输出大小当作真实设备传输量。最终图改为反转类别轴、取第一个最大值、映射回原索引，保持并列最大值的最后类别语义。最终加载预热记录确认 Gather、ArgMax、索引映射、概率/有效性归约均在 CUDA；识别网络仍有两个原有形状处理 Slice/Concat 在 CPU。此记录只覆盖加载预热，未把它称为稳态 GPU kernel profiling。

开关需要 CUDA 和包含额外角色的导入模型；普通模型仍走原路径。加载时保留原 Session，实验 Session 创建或预热失败不破坏原 Session。完整记录在 `test-results/compact-ctc.json`：本机 2K 中位 2658.50→2617.49 ms（约 1.5%），英文 462.67→489.40 ms，中日混合 553.64→500.69 ms，没有稳定的整体提速。2K 返回应用的输出为 197,802,120→42,288 字节；一个热运行样本的 CPU CTC 为 97.60→0.11 ms，但识别网络与新图处理仍占主要时间。测试文字和置信度一致，默认仍关闭。另有 7 项 CPU 子图检查、8 项实际 CUDA 边界检查及 6 项缺失/损坏实验图回退对照，均通过。

保留回退 Session，以及类别反转和有效性校验的中间张量，会增加内存/显存需求。报告保留运行后的资源快照，但未测独占 GPU 峰值；不把这条实验路径作为降低资源占用的成果，也不通过增开 Worker 换取跑分。

[ORT I/O Binding 文档](https://onnxruntime.ai/docs/performance/tune-performance/iobinding.html) 说明默认 CPU 输入/输出会引入设备拷贝。当前 GPU 归约在同一张识别图中执行，输出才回 CPU；这不等于实现了 GPU 输入缓冲、完整 GPU CTC 或零拷贝。WGC 读回、CPU 图像处理和 CPU 裁剪仍存在；独立 GPU I/O Binding/纹理链路尚未接入，没有虚构其加速数据。

开发环境安装 ONNX/Numpy 后，可在**新的目标目录**生成实验模型（Python 仅用于离线开发，发行程序不依赖它）：

```powershell
python scripts/create-compact-ctc.py .cache/test-models/ppocrv6-small.json .cache/optimized-models/compact-new
$env:GAMEOCR_COMPACT_DIR=(Resolve-Path .cache/optimized-models/compact-new).Path
$env:GAMEOCR_ORT_PROFILE_DIR=Join-Path (Get-Location) '.cache/compact-profile-new'
node scripts/benchmark-compact-ctc.mjs
node scripts/summarize-ort-profile.mjs .cache/compact-profile-new
python scripts/test-compact-graph.py .cache/optimized-models/compact-new/recognizer-ctc.onnx .cache/ctc-validation.onnx
```

在桌面设置中把模型目录指向该新目录，选择生成的 `Compact CTC (experimental)` 模型、CUDA 设备和 GPU 实验开关后加载。只开启开关但选择普通模型会继续原识别路径。实际 GPU 边界检查使用 `cargo run -p gameocr-onnx --release --example ctc_graph_check --features cuda -- .cache/ctc-validation.onnx docs/test-results/compact-ctc-cuda-validation.json`；需先按 testing.md 配置 CUDA Worker 的 `ORT_DYLIB_PATH` 与运行库 PATH。

## 5. 实验性共享内存

Windows page-file backed named mapping 替代单 Worker PPM 文件。每个 Worker 独占可复用 slot；NDJSON 控制协议保持不变，承载名字、容量、尺寸、frameId、generation、epoch、cache revision。映射名称包含宿主 PID、时间和序号，拒绝已有名称碰撞。写入完再发送请求；Worker 返回/终止前，独占请求借用阻止宿主覆盖。读取前后校验帧头，错误不会发布结果。

RGB 上限 64 MiB，slot 按容量增长且不超过上限；超限或创建失败回退 PPM 并显示原因。Worker 自己拷贝一份 RGB，保证 FrameCache 在宿主复用 slot 后仍有效。这是省去文件写读，**不是零拷贝**。释放、卸载和子进程终止由 RAII/OS 关闭句柄，分进程故障隔离保持；并行切片继续 PPM。

`test-results/shared-memory.json` 同条件实测：CPU 2K 静态时序 Executor 中位 18.42→11.70 ms，CUDA 17.48→11.79 ms；CPU 小对话框 4.80→3.27 ms。完整 OCR 的总耗时没有稳定收益，因此共享内存保留实验开关，未默认推广。取消和恢复测试包含在脚本中。

4K 独立对照保留完整小字分块：CPU PPM/共享内存中位 45.35/21.47 ms，CUDA 为 41.80/24.29 ms，输出一致；宿主传输阶段分别由 18.34→2.10 ms、12.73→2.73 ms。报告 `shared-memory-4k.json` 同时记录宿主和实际 Worker 的资源快照，包含取消后恢复。这些是静态复用帧的 Executor 延迟，仍不包括 WGC、预览或界面。

## 6. TensorRT RTX 可行性

已提供只读 `onnx-probe` / `scripts/probe-tensorrt-rtx.ps1`，区分编译包含的 EP、显卡条件和实际模型构建，探测不创建引擎缓存、不更换 DLL。[NVIDIA 官方要求](https://nvidia.github.io/TensorRT-RTX-EP-ABI/prerequisites.html) 为 Ampere 或更新的 RTX GPU，Turing 不支持；当前 GTX1650 因此不具备验证条件。ORT 目前推荐 [独立 EP ABI plugin](https://onnxruntime.ai/docs/execution-providers/TensorRTRTX-ExecutionProvider.html)，不能把传统 TensorRT EP 可用当作 TensorRT RTX 可用。

本轮依需求允许的范围停在兼容性调研和环境探测：没有集成未验证的 EP，也没有编造构建/稳态收益。RTX5060Ti 的可复现实验应另备兼容 plugin/runtime，固定 provider 版本；缓存键至少包含模型 SHA-256、GPU/compute capability、driver、ORT/plugin/TensorRT RTX 版本及精度/shape 配置。分别测 build、首帧、热态；先验证固定宽度桶，再覆盖动态 batch/宽度和精度。失败丢弃该缓存并回退原 CUDA/CPU Session，不能影响现有 CPU-only 包。

## 复现与限制

依 README 构建 Worker 和带 `test-fixture` 的 `executor-bench`；每个耗时脚本串行运行，不与编译或其他模型负载并发。`benchmark-latency.mjs`、`benchmark-temporal.mjs`、`benchmark-compact-ctc.mjs`、`benchmark-transfer.mjs` 各自生成独立 JSON。小样本 p95/p99 只用于复现，不能解读为生产长时间尾延迟承诺。全部默认仍为完整既有路径，RTX5060Ti、真实游戏视频、长时间稳定性和干净无驱动系统仍需单独验证。

本版推荐继续保持新开关默认关闭。若在这台 5600G/GTX1650 上试用固定位置 HUD，可选 Small、CPU、单 Worker、Temporal Auto，保留 1000 ms 完整刷新、30 次复用和 0.85 可信阈值；再按实际画面比较共享内存。不要为追求静态数字把刷新延长到桌面测试的 10 秒。优先补充真实游戏标注和自动捕获延迟分布，再考虑调整默认值；密集小字、复杂背景回退以及 GPU 识别网络本体仍是主要瓶颈。
