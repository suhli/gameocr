# 测试记录

多 Runtime 验证日期：2026-10-14 / 15。结果来自真实 ONNX / GGUF 文件和原生 Worker；单元测试中的合成概率图只验证算法，不作为真实 OCR 证据。完整文本、坐标、置信度和分阶段耗时保存在 [test-results](test-results)。

## 环境

Windows 11 x64 开发机，GTX1650 4 GB，NVIDIA 591.86，CUDA12.1，cuDNN9.8.0.87，ORT1.24.3，VS2022/MSVC14.41，Rust1.97.1，Node24.21.0，系统 WebView2。本机 llama.cpp CUDA 使用明确的旧工具链兼容开关及 SM75/80/86/89；CI 配置为 CUDA12.8.1、SM120，但尚未在远端执行。以下耗时有同时编译等背景负载，不是受控性能基准。

## 自动化与真实推理

- Svelte / TypeScript：0 errors、0 warnings；Vitest 2 项坐标映射测试通过；Vite 生产构建成功。
- `cargo test -p gameocr-core --no-default-features --locked`：13 项通过，覆盖 Factory 两类构造分派与扩展注册、设备候选、能力和参数验证、Manifest 迁移、旧历史 JSON、Grounding、变化检测和有界帧槽。
- `cargo test -p gameocr-onnx --locked`：3 项通过，覆盖 CTC blank/重复/混排、DB 空结果及缩放坐标还原、透视裁剪与纵向旋转。
- `cargo test -p gameocr --lib --locked`：12 项通过，覆盖多文件完整性/等长损坏/共享文件删除、导入回滚、真实 loopback HTTP 断点续传、原子配置、窗口身份、16 MiB 协议边界与截断响应拒绝。
- `scripts/test-onnx.mjs`：两规格各 4 张图，CPU 8 例 + CUDA 8 例均通过；还验证错误 JSON、未加载识别、错误 Runtime 拒绝、卸载、重载与进程正常关闭。CUDA Ready 之前检测、识别网络均通过实际 CUDA kernel 执行验证。
- `scripts/test-runtime-switch.mjs`：真实 CPU ONNX small → GGUF Q8_0 → ONNX medium，检查身份、坐标、GGUF 无伪造置信度和每个旧 Worker 退出，三次均通过。
- `scripts/desktop-smoke.mjs`：2026-10-15 的新 Release 主程序通过 23 项真实 GGUF 桌面回归，覆盖 WGC、CUDA Grounding、原始 PNG、bbox/文字联动、JSON/PNG 导出、去重、暂停帧强制重识别、缩放、推理中停止、最小化恢复、60 秒连续识别、窗口销毁、卸载和正常退出。子进程 PATH 仅保留 Windows 系统目录，CUDA 使用包内 DLL；[完整记录](test-results/desktop-gguf.json) 含被测 EXE 哈希。
- `scripts/desktop-runtimes.mjs`：真实 Tauri/WebView2 的 12 项检查通过，覆盖真实 CUDA 初始化失败后的 CPU 回退、显式 CUDA 错误、错误后恢复、加载状态参数锁、模型切换、能力驱动设置、两规格 WGC OCR 与匹配预览/JSON、ONNX 三文件导入、等长字典损坏和删除入口、错误网络回滚全部副本。新增强制终止测试应用自己的 ONNX 子进程：主程序继续存活、报告错误，重新加载后再次完成实际 OCR。

证据：[CPU 8 例](test-results/onnx-cpu-e2e.json)、[CUDA 8 例](test-results/onnx-cuda-e2e.json)、[跨 Runtime 切换](test-results/runtime-switch.json)、[完整便携版桌面 12 项](test-results/desktop-runtimes.json)。桌面报告记录被测 EXE SHA-256；最终便携版截图及 WGC 导出保存在本机 `artifacts/validation/portable-multi-runtime/`。

独立 CPU 便携包未附带 GPU Worker 或 NVIDIA DLL。子进程 PATH 仅保留 Windows 系统目录，通过 [ONNX 桌面 12 项](test-results/desktop-cpu-only.json) 和 [GGUF 桌面回归](test-results/desktop-cpu-gguf.json)：两种 Runtime 均实际识别 WGC 图像，Auto 在缺少 CUDA Worker 时回退 CPU，显式 CUDA 返回错误。此检查验证独立发行目录；不等同于无 NVIDIA 驱动机器的验证。

测试输入为本地生成的字体图片：英文 800×320；中日英混排含简体、繁体、日文；约 12° 旋转文字与 16px 小字；空白图。断言真实输出含 GAME OCR TEST、Health、Quest complete、游戏、繁體中文、こんにちは、ROTATED TEXT、Small game text 等关键词，同时检查 bbox、polygon 和置信度，未硬编码替换实际识别结果。

2026-10-14 16:52–16:53 一轮的 totalMs（不含模型加载）：

- small CPU：英文 164ms、混排 307ms、旋转/小字 166ms、空白 127ms。
- medium CPU：英文 590ms、混排 942ms、旋转/小字 637ms、空白 477ms。
- small CUDA：英文 507ms、混排 479ms、旋转/小字 432ms、空白 224ms。
- medium CUDA：英文 1247ms、混排 1591ms、旋转/小字 1442ms、空白 861ms。

GTX1650 上这批小图 CPU 更快。为解决该卡动态识别宽度的 cuDNN 不支持错误，目前使用默认 fallback 卷积算法；没有以理论 GPU 能力替代实测速度。small 与 medium 的混排均产生 4 个真实文字块；空白均返回 0 块。两种模型的完整字典有 18,708 项，不是测试字符子集。

GGUF CPU Plain 和 Grounding 已重新执行；v2 Worker 切换测试保留原始输出/解析行为，已有量化目录和本地导入流程继续可用。[CPU Plain](test-results/gguf-cpu-plain.json)、[CPU Grounding 多对象输出](test-results/gguf-cpu-grounding.jsonl)、[CUDA Grounding](test-results/gguf-cuda-grounding.json)。CUDA 回归返回 3 个真实文字框，单次约 1.83 秒。CPU Grounding 文件是 CLI `--repeat` 的多段 pretty JSON，扩展名历史沿用，不是每行一对象的严格 JSONL。

## 复现

先按 README 构建四套 Worker 并用 `fetch-onnx-models.ps1` 获取固定权重。CPU 不需要 CUDA DLL，也不需要 Python：

```powershell
$env:ORT_DYLIB_PATH=(Resolve-Path src-tauri/resources/workers/onnx/cpu/onnxruntime.dll).Path
$env:GAMEOCR_ONNX_WORKER=(Resolve-Path src-tauri/resources/workers/onnx/cpu/onnx-worker.exe).Path
$env:GAMEOCR_TEST_DEVICE='cpu'
node scripts/test-onnx.mjs
node scripts/test-runtime-switch.mjs  # 另需 models/ 下的已核对 Q8_0 + mmproj

$env:ORT_DYLIB_PATH=(Resolve-Path src-tauri/resources/workers/onnx/cuda/onnxruntime.dll).Path
$env:GAMEOCR_ONNX_WORKER=(Resolve-Path src-tauri/resources/workers/onnx/cuda/onnx-worker.exe).Path
$env:GAMEOCR_TEST_DEVICE='cuda'
$env:PATH=(Resolve-Path src-tauri/resources/workers/cuda-runtime).Path+';'+$env:PATH
node scripts/test-onnx.mjs
# UI 始终按自己的 Worker 目录选择 DLL；不要把 CLI override 留给 UI
Remove-Item Env:ORT_DYLIB_PATH
cargo build -p gameocr --features test-fixture --bins
node scripts/desktop-runtimes.mjs
```

桌面脚本自行启动 Vite（先等待可访问）和专用数据目录的测试应用，创建自有 Win32 文本窗口，不操作现有游戏或日常模型目录。Vite 排除 `.cache`、模型及构建/导出目录，避免监视正在复制的 ONNX 临时文件触发 EBUSY。指定 `GAMEOCR_APP` 为便携 EXE 时不启动 Vite；`GAMEOCR_TEST_OUTPUT` 指定截图/数据目录，`GAMEOCR_TEST_REPORT` 指定 JSON 报告文件。需要可交互桌面，headless CI 不伪造此测试。

## 分发验收

四套 Release Worker 已实际编译，ORT CPU/GPU 和 cuDNN 归档 SHA-256 已核对。原生依赖检查使用 PE 普通及延迟导入表，只在包内目录和 Windows System32 解析；检查 UI 不链接推理库、CPU 不依赖 CUDA。

增加 cuDNN 后，原始 Tauri NSIS 整体压缩在约 2 GiB 原始数据处出现 `error mmapping file ... is out of range`。已基于锁定的 Tauri CLI 2.12.2 模板改用逐文件 LZMA；WiX 改为自动分割、嵌入式 1024 MiB CAB。保留 upstream 安装/卸载逻辑与 MIT 许可，详见 [模板说明](../src-tauri/installer/README.md)。不删减任何已验证的 Worker 或 CUDA 运行库来绕过限制。

2026-10-15 最终 Release 构建成功，文件位于本机 `artifacts/`：

- 完整便携 ZIP：1,717,779,934 字节，约 1.72 GB；86 个文件逐一对比解压流 SHA-256，与实际测试目录完全一致；75 个 x64 PE 文件的普通/延迟导入解析通过。
- 独立 CPU ZIP：16,584,041 字节，约 16.6 MB；45 个文件逐一比对通过，35 个 x64 PE 文件导入检查通过，未包含 GPU Worker 或 NVIDIA DLL。
- NSIS：933,200,492 字节，约 933 MB；MSI：1,693,249,536 字节，约 1.69 GB。安装器归档检查结果、四个文件的 SHA-256 和对应桌面报告见 [分发校验记录](test-results/package-validation.json)。

本节记录的完整与 CPU 便携版均已从当时的打包目录启动并完成上述真实桌面验证。2026-10-16 的便携包已由下方并行增量版本替换；本节大小及旧 `SHA256SUMS.txt` 仅对应历史发行版。GitHub Actions 将发行包和 CPU 端到端报告放在同一个上传根目录，标签发布可直接使用下载后的文件列表。

安装文件生成及归档校验不代表已经在干净 Windows 上安装/卸载；本机 CUDA 包使用 CUDA12.1 / SM75/80/86/89，RTX5060Ti 应另行使用 CI 中配置的 CUDA12.8.1 / SM120 构建并实测。

## 原 GGUF 基线记录（2026-10-12 / 13）

旧版已真实执行 WGC、CUDA Grounding、预览/坐标联动、JSON/PNG 导出、变化去重、暂停/恢复、窗口缩放、最小化报错、推理中停止丢弃旧结果、窗口销毁、卸载和正常退出。2026-10-13 的 Release 长测实际完成约 1809 秒 / 176 次采样，历史上限 5；主进程私有内存约 66–70 MiB，worker 约 2439–2991 MiB。证据为本机 `artifacts/validation/desktop-release/{report,summary,soak-progress}.json`。

这些是重构前 GGUF 基线，不能当作新版 ONNX 30 分钟内存测试。更早约 700 秒中断的一轮仍按中断处理，不用它替代完整长测。原 README 中待完成的旧记录已根据现存报告纠正。

## 可选并行增量验证（2026-10-16）

本轮基于 `feature/multi-ocr-runtime` 的并行实现，未修改已有 CUDA 算法兼容设置。使用合成截图替代真实游戏画面，来源、完整六模式数据及优化取舍见 [并行报告](parallel-performance.md)。

- Rust：core 16、desktop lib 14、ONNX pipeline 3 项通过；包含旧 settings/history 兼容、切片覆盖/检测预算、坐标/去重、Worker 协议边界与导入模型显存门槛。前端 svelte-check 0 错误/警告、Vitest 2 项与 Vite production build 通过。
- 真实 Small/Medium CPU 8 + GTX1650 CUDA 8 例通过；原多 Runtime 桌面 12 项通过。本轮 GGUF Q8_0 在打开 ONNX 并行设置时仍走单 Worker，真实文字/坐标及空置信度回归通过。
- [Executor 集成](test-results/parallel-integration.json) 12 项通过：两 CPU 常驻与分摊线程、空白/边界处理、取消后可复用、推理中终止本测试的 Worker 后由另一实例重试完整帧、CPU-only 目录 Auto 回退、显式 CUDA 错误、零 Worker 拒绝、四 CPU 线程预算、卸载清理。
- CPU×2 持续运行实际 182.63 秒 / 216 帧。10 帧预热后，主进程与两个 Worker 私有内存合计约 117.2 MiB → 116.6 MiB（变化 -0.63 MiB）；每帧任务目录为空、完成后队列清空，所有测试拥有的 Worker 退出。此结果不是数小时压力测试。
- 导入模型显存准入修正后的 [最终 Executor 回归](test-results/parallel-integration-final.json) 再次通过全部 12 项；CPU×2 持续 181.966 秒 / 200 帧，预热后私有内存合计变化 +0.61 MiB，任务目录和自有进程清理通过。这是两次独立的约三分钟检查，不是六分钟连续长测。
- [并行桌面](test-results/desktop-parallel.json) 5 组通过：设置 UI/持久化、实际 1866×1013 WGC 四片合并、对应预览与 JSON/PNG、暂停/停止/目标重选时拒绝迟到结果、卸载及切换回单 Worker、推理中正常关闭应用后子进程和本会话目录清空。
- Release 应用构建通过。新加 `scripts/benchmark-parallel.mjs`、`scripts/test-parallel.mjs`、`scripts/desktop-parallel.mjs` 与开发专用 `executor-bench`；CI 接入 CPU 并行回归，但本轮没有远端 CI 执行记录。
- 最终 [CPU 便携包](test-results/desktop-parallel-cpu-portable.json) 与 [完整便携包](test-results/desktop-parallel-portable.json) 各通过 5 组真实桌面检查，均在不启动 Vite 的情况下加载内嵌前端，以 CPU×2 完成 WGC 识别、停止/切换和推理中正常退出清理。两包主程序 SHA-256 一致，匹配正式 Tauri 发布构建；包内 CUDA 推理的证据来自前述独立 Worker 回归与六模式基准。
- [本轮分发校验](test-results/parallel-package-validation.json)：CPU ZIP 为 16,721,331 字节，45 个文件逐一比对、35 个 x64 PE 依赖检查通过；完整 ZIP 为 1,717,959,910 字节，86 个文件逐一比对、75 个 PE 依赖检查通过。当前校验和分别为 `artifacts/SHA256SUMS-cpu.txt` 与 `artifacts/SHA256SUMS-portable.txt`。NSIS/MSI 未重新构建，仍为上一轮版本，不包含本次并行升级。

自动化脚本操作隔离的测试窗口与本测试拥有的进程。桌面测试每次使用独立数据目录，以避免旧的强制结束测试留下的会话影响正常退出校验。构建日志、截图与测试可执行文件保留于本机 `.cache`、`artifacts/validation/parallel-desktop`、`target`。

## 自适应调度增量验证（2026-10-19）

实现、测量口径和完整六模式结果见 [自适应调度报告](adaptive-scheduling.md)。默认保持 Single，历史设置自动迁移，未证明跨场景总体稳定加速。

- Rust core 22、desktop lib 16、ONNX pipeline 3 项通过；svelte-check 0 错误/警告、Vitest 2 项与生产前端构建通过。新增集中布局在切换冷却中及时返回单实例的回归。
- [发布版 Adaptive 集成](test-results/adaptive-integration.json) 9 项通过：真实特征、总线程预算、24 帧中实际 1 次探索、取消后不学习、重启只读历史、Batch 指纹隔离、清空/损坏恢复、CPU 上限、终止本测试 Worker 后只记录失败、显式 CUDA 拒绝。
- [六模式基准](test-results/adaptive-benchmark.json) 完整完成。8 张学习图与 7 张评估图文件哈希不重合；每模式 59 帧，合计 354 帧均 CER=0、精确行召回=100%，但仅代表该合成语料。1080p HUD 特征中位 1.81 ms、P95 2.10 ms。
- 自适应在本轮评估避免全部整图复验；固定 CPU×2 为 35/59 帧。已有历史的三组均选择过 CPU×1/CPU×2，新风格画面通过 workload 匹配复用经验，未知中心布局保守选择单实例。整体收益不稳定，尤其交替布局仍受重载成本影响，不声称普遍加速。
- 独立 CPU 争用实验实际触发 2 次历史重新校准，测试负载停止后延迟恢复，争用期没有探索。[严格重建途中取消测试](test-results/adaptive-switch-cancellation.json) 验证新池 PID 确实更换、两实例仍可用、取消不更新统计、后续 OCR 成功。
- 原生 Small/Medium [CPU 8 例](test-results/adaptive-onnx-cpu.json)和 [GTX1650 CUDA 8 例](test-results/adaptive-onnx-cuda.json)通过，包含 IPC 错误、卸载/重载和 Worker 替换；并发构建期间的这些用例用于正确性回归，不用于性能对比。
- [原 Executor / GGUF 兼容回归](test-results/adaptive-compatibility.json) 13 项通过。GGUF 在 Manual 和 Adaptive 设置下均保持单实例和空置信度；CPU×2 连续 181.475 秒 / 202 帧，私有内存变化 +880,640 字节（约 0.84 MiB），临时文件、队列与自有子进程清理通过。持续检查期间允许生产构建并行，因此不将其吞吐当作纯 OCR 基准。
- [开发桌面测试](test-results/desktop-adaptive.json) 8 组通过，覆盖手动设置、真实 WGC 四片结果与导出、停止/目标变化、模型卸载、自适应默认值与居中设置框、暂停不学习、分析/推理中清空历史和正常退出清理。该报告记录此前开发构建的哈希。
- 正式 Tauri 构建后的 [CPU-only 便携包](test-results/desktop-adaptive-cpu-portable.json)和[完整便携包](test-results/desktop-adaptive-portable.json)各通过上述 8 组，使用内嵌前端、没有 Vite。[CPU 包多 Runtime 桌面 12 项](test-results/adaptive-desktop-runtimes.json)也通过，覆盖两规格真实 WGC、导入/损坏/删除、Auto 回退、显式 CUDA 错误、Worker 崩溃隔离及恢复。
- [原 CUDA Executor 回归](test-results/adaptive-cuda-executor.json)通过：新版 Manager 的 Single 使用一个真实 CUDA Worker，Manual 使用 GPU×2，均识别完整 HUD；这是 GTX1650 正确性回归，不是 Adaptive GPU 探索。可在具备 NVIDIA 环境时运行 `node scripts/test-cuda-executor.mjs`。
- [本轮发行校验](test-results/adaptive-package-validation.json)：CPU ZIP 16,864,172 字节，45 个文件/35 个 x64 二进制依赖校验通过；完整 ZIP 1,718,102,751 字节，86 个文件/75 个依赖校验通过。两包主程序 SHA-256 均为 `b42a55008efae8481aa2b9882750d0919e7448bf47824db3c777150b1380df36`，与桌面报告一致。NSIS/MSI 未重新构建，现存安装版不包含 Adaptive。
- CI 加入真实 Adaptive CPU 回归及报告上传。本轮未运行远端 GitHub Actions。

## 六项识别效率优化（2026-10-20–21）

实现与质量／速度取舍见 [效率优化说明](ocr-efficiency.md)。默认关闭 ROI、小字分块、增量复用、宽度分桶与选择性补检；模型仍为 FP32。以下为本机合成语料和自有窗口测试，不代表实际游戏的整体准确率。

- Rust core 23、ONNX 11、desktop lib 16 项通过；前端检查 0 错误／警告、Vitest 2 项通过。
- 最新 Small／Medium 原生 [CPU 8 例](test-results/efficiency-onnx-cpu.json)与 [CUDA 8 例](test-results/efficiency-onnx-cuda.json)通过，包括错误 IPC、卸载／重载、空白、多语言与 Worker 替换。执行时允许桌面编译，因此这些报告用于正确性，不用于性能比较。
- [CPU 增量＋宽度分桶](test-results/efficiency-incremental-cpu.json)通过：4K 的重复、单数字变化、删除、滚动与无缓存文本／坐标一致，均 CER=0、召回完整；来源切换、取消后刷新及临时文件清理正常。
- 对应 [CUDA 增量＋分桶](test-results/efficiency-incremental-cuda.json)通过相同序列和失效检查。[CUDA 小字／ROI](test-results/efficiency-small-text-cuda.json)在 2K / 4K 分别完整召回 53 行及 4 行，无额外框。[CUDA 分桶基准](test-results/batching-cuda.json)质量不变，但 GTX1650 总耗时 2389→2687 ms，因此保留默认关闭。
- 最新 [Adaptive 9 项](test-results/efficiency-adaptive.json)与 [Executor／GGUF 13 项](test-results/efficiency-compatibility.json)通过。此次持续 CPU×2 检查约 31 秒／34 帧，用于回归，并不替代前述三分钟检查或长期压力测试。
- [低精度评估](test-results/precision.json)完成 4 个配置 × 5 个场景，每场景预热 1 次、测量 3 次。2K／4K 测量文本全部正确，英文／中日繁体关键词通过，空白无输出；INT8、混合 FP16 均未比对应 FP32 提速，保留独立实验模型。转换脚本校验模型和字典，拒绝覆盖已有输出；桌面没有 Python 依赖。
- CI 加入完整小字／ROI 与增量＋分桶 CPU 检查及报告上传；本轮没有远端 CI 执行记录。低精度评估不加入默认发布流程。
- [CPU 便携包桌面 11 组](test-results/desktop-efficiency-cpu-portable.json)通过，新增设置持久化、居中、完整分块与补检互斥控件、真实 WGC 的 PPM 输入、增量复用及自动 OCR 单数字变化。[CPU 包运行时 12 组](test-results/efficiency-desktop-runtimes.json)通过模型切换、损坏／删除、显式 CUDA 错误、Auto 回退、崩溃隔离和恢复。
- [完整便携包桌面 11 组](test-results/desktop-efficiency-portable.json)同样通过，额外明确断言数字改为 101 后最新结果不含旧值 100。两包均使用内嵌前端，没有启动 Vite；包内 ONNX Worker 哈希与精度基准实际使用的二进制一致。
- 2026-10-21 [最终分发校验](test-results/efficiency-package-validation.json)：CPU ZIP 16,920,304 字节，45 个文件／35 个 x64 依赖检查通过；完整 ZIP 1,718,202,975 字节，86 个文件／75 个依赖通过。两包主程序 SHA-256 为 `6a743c7419003e462fd1ef17a29dc77f1d18940c9c6e1eae76b4bad313d14853`，与桌面报告一致；ZIP 每个文件均逐一核对。NSIS／MSI 未重建，现存安装包不包含这次升级。

复现时先按 README 构建 Worker 和 `executor-bench`，不同耗时基准应单独运行，不与编译、其他 OCR 或 GPU 负载并发。`test-incremental.mjs` 支持 `GAMEOCR_TEST_DEVICE`、`GAMEOCR_WIDTH_BUCKETS=1`、`GAMEOCR_INCREMENTAL_REPORT`；`test-small-text.mjs` 支持设备、场景、模式、次数和报告路径覆盖。

## 未验证和已知限制

- RTX5060Ti / SM120、其他显卡，以及 CUDA12.8.1 配置的远端 GitHub Actions；目前硬件验证仅 GTX1650。
- 干净 Windows11、完全无 NVIDIA 驱动机器、安装版系统安装/卸载、签名。PATH 隔离和 CUDA_VISIBLE_DEVICES=-1 只能验证本机子进程路径及故障回退，不能冒称干净机器通过。
- 本轮未重新验证真实 Android / 模拟器 ADB 捕获与断开重连；原测试中只枚举过 ADB，当时设备为空。WGC/ADB 实现未在此重构中替换。
- DirectX/OpenGL/Vulkan 游戏、受保护或反作弊画面、HDR、多显示器不同 DPI；程序不绕过保护。
- Q4_K_M、Q5_K_M、F16 的完整质量/性能对比；目录元数据保留，真实 GGUF 回归采用 Q8_0。
- ONNX 数小时压力和广泛真实游戏准确率评估、任意角度/弯曲文本、竖排中日文、其他 ONNX/GGUF 架构。三分钟合成语料检查不替代这些验证。现 Adapter 只验证指定官方网络和完整字典。
- 模型列表每次扫描文件 SHA-256，大型 GGUF 或慢盘有等待时间；后台线程不会阻塞 UI 事件循环。
- 大型 cuDNN/CUDA 运行库仍占较多发行空间，已跨 Runtime 共享一份；完整 ZIP、NSIS、MSI 均含 CPU/CUDA，另提供不含 NVIDIA 运行库的独立 CPU ZIP。CPU 包的 Auto 模式显示回退原因后使用 CPU，显式 CUDA 报告缺少 Worker。

停止 OCR 立即取消结果发布，进入原生库的调用会完成或在 180 秒超时后终止。UI 显存遥测不可用；单 Worker 引擎 totalMs 不含捕获、传输前 PNG 编码、结果 JPEG 和 UI 绘制。新增 execution.endToEndMs 包含帧传输、任务调度与合并，桌面值另含结果 JPEG；并行 totalMs 使用 Executor 墙钟，不能用各 Worker 阶段之和代替。
