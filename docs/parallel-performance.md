# PP-OCRv6 并行验证

本轮测试日期：2026-10-16。用户同意先不用真实游戏截图，本报告使用仓库内可复现的合成画面；不能将结果当成真实游戏准确率或其他 GPU 的性能结论。

## 配置与实现

默认 `enableParallelInference=false`、`enableCpuWorkers=false`、`gpuWorkerCount=1`、`cpuWorkerCount=1`。关闭并行恢复旧单 Worker 请求与推理参数；llama.cpp 忽略 ONNX 并行开关。

CPU 只启动 CPU Worker，CUDA 只启动 GPU Worker且失败不回退。Auto 可以混合；GPU 全部不可用时仍回退 CPU，部分不可用则显示实际启动数量。配置数与实际数都保留在原始 benchmark 报告中。并行设置修改前必须卸载模型。

主要实现：`crates/gameocr-core/src/parallel.rs` 负责配置、FrameInput、切片与结果几何；`src-tauri/src/executor.rs` 负责独立进程池、动态队列、重试与计时；`service.rs` 保留捕获/历史和取消边界；`gameocr-onnx` 保留具体图像和推理实现。`ExecutionStats.svelte` 展示统计。完整生命周期及协议见 [架构说明](architecture.md#可选并行执行器)。

## 方法与环境

Windows 11（10.0.26200），Ryzen 5 5600G / 12 逻辑核，GTX 1650 4 GiB，驱动 610.88。CPU 配置总线程预算 4；CPU×2 每 Worker 2 线程，混合 GPU Worker 的 CPU 侧线程为 1。ONNX Runtime 1.24.3、CUDA 12.1、cuDNN 9.8.0.87，官方 PP-OCRv6 Small FP32 模型与完整字典。CUDA 初始化通过两个网络的真实 kernel profile 验证。

使用 Release `executor-bench` 调用与桌面相同的 Executor 和已构建 Release Worker。每模式模型只加载一次，记录冷启动；每个场景先预热 2 次，再测 5 次，中位数与 nearest-rank P95（5 次时等于最大值）都保留。此样本数不足以推断稳定的尾延迟分布。

计时从已解码 RGB 帧进入 Executor 开始，包含文件传输、切片、排队、所有切片/重试推理、合并及临时文件清理；不包括测试语料 PNG 解码、截图采集、UI JPEG 或绘制。桌面的 execution.endToEndMs 另含结果 JPEG 生成。并行原生各阶段之和表示工作量，不能当作帧延迟。

8 个场景包含 1080p/1440p 四角 HUD、单对白框、20 行菜单、中日英混排、旋转/竖向英文、空白、横跨切片边界长句。检测最大边默认 960，Batch=6，其余阈值沿用旧默认值。识别准确率按 NFKC、忽略空白与大小写后评估，同时记录文本行召回、字符错误率及额外块。bbox 真值是字体布局矩形，非逐字紧框，IoU/中心偏差仅作坐标回归参考；不以标签参与识别。

GPU 采样来自 nvidia-smi，含同卡其他应用。CPU 占用由测试主进程与 Worker 累计 CPU 秒之差除以运行时间、逻辑核心数计算；资源快照还包括私有内存、工作集、峰值工作集。共享桌面环境没有排除系统及其他应用干扰；测试期间本任务不并行编译或跑其他推理测试。

## 主矩阵结果

以下每行按「CPU×1、GPU×1、CPU×2、GPU×2、GPU1+CPU1、GPU2+CPU2」排列，单位 ms，数值为中位数 / P95。所有六种配置均成功启动了请求数量的实例，默认检测边长 960、Batch=6。

- 1080p HUD：241.9/291.2；468.0/543.5；216.4/249.2；1637.3/1705.4；229.4/246.2；216.4/268.0。
- 1440p HUD：286.3/307.6；489.1/526.2；207.9/226.7；1611.7/1649.2；257.3/278.4；213.7/282.3。
- 对白框：214.1/475.5；460.8/505.3；273.1/277.6；496.2/524.0；221.5/224.5；347.6/452.5。
- 密集菜单：713.1/874.8；1176.6/1199.1；1460.7/1576.9；3097.5/3318.5；1303.3/1370.0；1541.8/1785.0。
- 中日英混排：283.2/302.8；483.4/600.0；197.9/208.9；1625.3/1633.9；270.0/332.1；231.5/262.3。
- 旋转文字：204.3/250.1；470.4/509.9；185.7/244.4；1436.1/1569.8；243.5/292.3；205.8/224.5。
- 空白：159.8/165.4；263.7/281.9；205.9/231.8；277.2/292.0；185.7/189.3；339.5/421.3。
- 跨边界长句：353.1/402.6；497.4/507.9；727.2/846.1；2184.8/2244.2；730.0/749.8；951.0/1024.1。

CPU×1 是这些场景中较快的单 Worker 基线。CPU×2 在 HUD 1080p、1440p、混排、旋转场景分别为 1.12×、1.38×、1.43×、1.10×；菜单和跨界长句由于边界整图复验分别降至约 0.49×。GPU×2 在此 GTX1650 环境明显更慢，不应据此配置默认双 GPU 实例。混合池预热后通常把这些场景交给 CPU，不能把上述混合模式成绩称为 CPU 与 GPU 同时推理的加速。每帧实际 CPU/GPU 任务数见 JSON。

六模式全部测量轮次，规范化后文本行召回 100%、字符错误率 0、无额外文字块，空白结果为空。按非空场景等权计算，单 Worker 平均 bbox IoU 约 0.819、中心偏差 7.7 px；切片模式约 0.836 / 7.6 px。坐标真值仅字体布局近似，不能将 IoU 的差异当作真实游戏精度提升。跨边界完整句子由真实整图复验得到，没有拼接假文字或补造置信度。

同顺序冷启动约 278、1057、595、2318、1387、3472 ms；场景运行结束后的主进程+Worker 私有内存合计约 58.9、1397.7、116.4、2979.1、1252.8、2363.3 MiB。它是主机私有内存，不等于显存。整卡显存采样峰值约 2010、2483、2015、3088、2291、2488 MiB，包含其他应用，不能全部归因于 OCR。测试进程平均 CPU 占整机约 27.8%、8.0%、23.4%、14.0%、34.4%、21.0%；各场景真实 framesPerSecond 保存在 JSON，未用中位数倒数冒充平均吞吐。

## Batch、分辨率与传输对照

[调参原始数据](test-results/parallel-tuning.json) 包含四种模式 × 五套参数 × 两个场景，每场景 2 次预热 + 5 次测量，所有轮次规范化字符错误率为 0。主矩阵与调参矩阵分开运行，其基线各自在同组中比较，不能挑选跨组最有利数字来计算加速。

- GTX1650 单 Worker 菜单，Batch=1/6/12 的中位数分别约 4289/1211/873 ms；Batch=12 的 P95 约 1445 ms，高于 Batch=6 的 1227 ms。CPU 单 Worker 同场景约 596/612/642 ms，差异较小。保留默认 6 与用户显式设置，不按 GPU 或 Worker 数自动覆盖。
- CPU 单 Worker 1080p HUD，检测边长 640/960/1280 的中位数约 171/236/445 ms；CPU×2 约 129/176/297 ms。当前 34–48 px 合成文字没有漏识别，不能据此保证真实小字在 640 下的召回；默认仍为 960。
- 主矩阵纯色背景中，PNG 编码中位数通常 2–5 ms，PPM 并无普遍收益。额外 [纹理传输压力测试](test-results/parallel-transport.json) 使用固定种子的低对比噪声，CPU 单 Worker PNG 传输 18.13 ms，PPM 3.39 ms；端到端分别 313.8 ms 和 261.9 ms（1.20×）。CPU×2 切片为 210.7 ms（相对同组 PNG 基线 1.49×）。三者四行文字均完整，未触发边界复验。该纹理是专门用于压缩开销测试的合成数据，不是真实游戏画面。

最初补测曾和调参最后一组短暂重叠，受影响数据已弃用；纹理测试与最后一组调参重新串行执行，调参 JSON 记录该组重跑时间。此后才进行构建/打包。

## 原始数据与复现

- [六种模式及传输对照](test-results/parallel-benchmark.json)：完整逐帧延迟、坐标、文字、准确率、任务分配、冷启动与资源采样。
- [集成与持续运行](test-results/parallel-integration.json)：真实 Worker 取消、故障、CPU 回退、GGUF 及进程清理。
- [桌面并行回归](test-results/desktop-parallel.json)：真实 WGC、UI 持久化、历史、JSON/PNG、目标切换与退出。

```powershell
# 先按 README 构建 ONNX CPU/CUDA Worker 并取得真实模型
./scripts/create-parallel-fixtures.ps1
cargo build -p gameocr --features test-fixture --bin executor-bench --release --locked
node scripts/benchmark-parallel.mjs

# 加测单实例 PPM 传输路径（GPU 对照还改变 CPU 侧线程数）
$env:GAMEOCR_BENCH_ATTRIBUTION='1'
node scripts/benchmark-parallel.mjs
Remove-Item Env:GAMEOCR_BENCH_ATTRIBUTION

# 同场景比较 Batch=1/6/12、检测边长=640/960/1280
$env:GAMEOCR_BENCH_TUNING='1'
$env:GAMEOCR_BENCH_INPUTS='hud-1080,dense-menu'
$env:GAMEOCR_BENCH_MODES='cpu-1,gpu-1,cpu-2,mixed-1-1'
$env:GAMEOCR_BENCH_REPORT='docs/test-results/parallel-tuning.json'
node scripts/benchmark-parallel.mjs

# 单独生成带纹理的传输压力图（与其他基准串行执行）
./scripts/create-parallel-fixtures.ps1 -Directory .cache/parallel-textured -TexturedOnly
$env:GAMEOCR_BENCH_CORPUS='.cache/parallel-textured/corpus.json'
$env:GAMEOCR_BENCH_ATTRIBUTION='1'
$env:GAMEOCR_BENCH_MODES='cpu-1,cpu-2,cpu-1-raw'
$env:GAMEOCR_BENCH_REPORT='docs/test-results/parallel-transport.json'
Remove-Item Env:GAMEOCR_BENCH_TUNING,Env:GAMEOCR_BENCH_INPUTS -ErrorAction SilentlyContinue
node scripts/benchmark-parallel.mjs

node scripts/test-parallel.mjs
cargo build -p gameocr --features test-fixture --bins --locked
node scripts/desktop-parallel.mjs
```

`GAMEOCR_BENCH_CORPUS` 可指定同 schema 的自有图片/标注语料。`GAMEOCR_BENCH_ROUNDS` 为 3–100。`GAMEOCR_EXECUTOR_BENCH` 可切换执行器路径，正式性能报告应使用 Release。测试数据目录都在 `.cache` / `artifacts`，不修改日常应用设置。集成测试需要单独下载的 GGUF Q8_0 + mmproj；CI 显式 `GAMEOCR_SKIP_GGUF=1` 时报告跳过，持续测试也缩短为 30 秒，不能替代本机结果。

`GAMEOCR_BENCH_PARAMETERS` 可按 `6:1280` 等逗号分隔值筛选参数组合。完成特定补测后清除相应 `GAMEOCR_BENCH_*` 环境变量再运行完整矩阵。性能数据来自最终显存准入修正前的执行器；修正只让导入模型按权重文件大小获得正确门槛，此处内置 Small 的 512 MiB 门槛与所有帧处理代码均未改变。

## 优化取舍

并行路径采用独立 PPM 文件和源图逐行写入，省掉 PNG 压缩及裁剪 RGB 分配；保持进程隔离。CPU 单实例 PPM 对照用于量化传输影响，GPU 对照还把 CPU 侧线程从 4 改成 1，因此不能把 GPU 的全部变化归因于文件格式。

保留已有按宽高比分组的批量识别与两个常驻 Session。ORT 默认顺序图执行、全部图优化，未启用并行图执行或额外 inter-op 线程池。多 CPU 实例分摊 intra-op 预算，避免每实例按所有逻辑核创建线程。ORT 的自旋行为没有在本轮单独做控制实验，保持原设置，不宣称调参收益。官方资料：[线程管理](https://onnxruntime.ai/docs/performance/tune-performance/threading.html)、[图优化](https://onnxruntime.ai/docs/performance/model-optimizations/graph-optimizations.html)。

保留 GTX1650 已验证的 cuDNN `ConvAlgorithmSearch::Default` 和 `conv_max_workspace=false`。没有启用 FP16、TensorRT、共享内存、额外预处理线程或 ROI 结果缓存。动态尺寸和识别文本量仍影响 CUDA 耗时；首轮模型 warmup 与每场景 warmup 分别记录/排除。

## 适用范围

并行是可选策略，默认保持关闭。高文字密度及跨界框可能触发完整性整图复验，从而比单 Worker 更慢；同一卡上的两个 GPU 进程也可能争抢资源。调度器可以让已测得较慢的 GPU 等待，由 CPU 完成后续任务；因此混合池存在不代表每一帧都同时使用两类设备，必须看实际任务数。

本轮只覆盖 Small 的并行性能矩阵；Medium 已做默认 CPU/CUDA 真推理与模型切换回归，没有完整多实例性能矩阵。长测约三分钟，不能声称数小时稳定。真实游戏、竖排中日文、HDR/反作弊捕获、RTX5060Ti、其他 GPU、无 NVIDIA 驱动的干净系统、真实 ADB 设备、安装器安装/卸载仍需各自验证。历史安装器来自前轮构建，不能作为本轮并行功能的安装验收。
