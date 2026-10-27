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
- GPU 组件仍需要较多下载量和本地磁盘空间；标准 ZIP 和安装器配置已改为首次显式使用时下载，完整离线 ZIP 可选。CPU 包保留离线识别，Auto 显示回退原因后使用 CPU，显式 CUDA 提示更换支持下载的标准包。当前布局见下方 2026-10-23 验证记录。

停止 OCR 立即取消结果发布，进入原生库的调用会完成或在 180 秒超时后终止。UI 显存遥测不可用；单 Worker 引擎 totalMs 不含捕获、传输前 PNG 编码、结果 JPEG 和 UI 绘制。新增 execution.endToEndMs 包含帧传输、任务调度与合并，桌面值另含结果 JPEG；并行 totalMs 使用 Executor 墙钟，不能用各 Worker 阶段之和代替。

## 实时 OCR 的独立验证入口

新增 `benchmark-latency.mjs`（无缓存基线）、`benchmark-temporal.mjs`（时序逐帧 GT/Full 对照）、`benchmark-compact-ctc.mjs`（CUDA 字符/置信度一致性）、`benchmark-transfer.mjs`（PPM/共享内存与取消恢复），报告分别在 `docs/test-results/`。测试应串行执行，不能与编译同时测性能。

时序脚本默认 CPU；`GAMEOCR_TEST_DEVICE=cuda` 切换设备，`GAMEOCR_TEMPORAL_SCENE` 筛选场景，`GAMEOCR_TEMPORAL_REPORT` 指定报告，`GAMEOCR_TEMPORAL_BUCKETS=1` 比较宽度桶，`GAMEOCR_TEMPORAL_BATCH` 设置批大小，`GAMEOCR_TEST_MODEL=ppocrv6-medium` 切换模型。`GAMEOCR_TEMPORAL_RESUME=1` 仅跳过已完成五策略的场景，并拒绝不同 Worker/语料哈希、模型或批处理配置的报告；改变实现后应重新运行，不把旧结果当新结果。`GAMEOCR_TEST_TEMPORAL=1 node scripts/desktop-parallel.mjs` 增加真实 WGC、UI、暂停/恢复、Adaptive 单实例和 Worker 崩溃重载检查。

Windows CI 加入真实 CPU 时序五策略回归和报告上传；新字延迟统计用 `test-temporal-measurements.mjs` 覆盖即时响应、延迟恢复、未恢复即被替换和无变化情况。远端 CI 本轮未执行。`test-ctc-fallback.mjs` 用真实原始模型验证 CPU/CUDA 在实验图缺失或损坏时继续识别。`GAMEOCR_TRANSFER_4K=1` 与 `GAMEOCR_TRANSFER_REPORT` 可单独比较 4K 的 PPM/共享内存；保留完整小字分块。

真实序列可用 `scripts/temporal-annotator.html` 标注，再由 `import-temporal-corpus.mjs` 导入。没有实际游戏语料时只报告合成数据；截图时间与脚本送帧时间分开说明。详见 [完整测量口径](latency-optimization.md)。

## 实时 OCR 最终回归（2026-10-22）

- Rust core 24、ONNX 14、desktop lib 17 项通过，合计 55 项；格式检查通过，前端检查 0 错误/警告、Vitest 2 项通过。
- [CPU](test-results/temporal-cpu.json) 与 [CUDA](test-results/temporal-cuda.json) 各 18 个场景、五种策略、520 帧，均无相对 Full 的文字/位置差异，GT CER 0、精确行召回完整。渐隐、阴影和纹理变化的语料保持同一 generation，验证实际风险检测而非依赖来源切换。分桶 120 帧、Medium HUD 60 帧的专项也通过，效果和未达标边界单独报告。
- 原生 Small/Medium [CPU 8 例](test-results/temporal-onnx-cpu.json)、[CUDA 8 例](test-results/temporal-onnx-cuda.json)及 [Executor/GGUF 13 项](test-results/temporal-compatibility.json)通过。此次持续 CPU×2 检查为 32.73 秒/32 帧，预热后私有内存合计变化 +0.56 MiB，临时文件、队列和自有进程清理通过；不替代先前三分钟检查或长期压力测试。
- CUDA CTC 的 7 项 CPU 子图检查、[8 项实际 CUDA 边界检查](test-results/compact-ctc-cuda-validation.json)、[6 项原始图回退检查](test-results/compact-ctc-fallback.json)通过。[Provider 记录](test-results/ctc-provider-profile.json)确认最终图的 ArgMax/归约在 CUDA 执行；仅覆盖加载预热。早期 CPU ArgMax 回退实验另存报告，不用于声称设备传输减少。
- [CPU 便携包桌面 13 组](test-results/desktop-temporal.json)与 [模型/运行时 12 组](test-results/temporal-desktop-runtimes.json)通过，使用内嵌前端。时序检查明确从 Manual 设置启动并断言实际单 Worker，独立于旧增量/分桶/补检开关；新增共享内存、严格预览复用、暂停恢复和崩溃重载生命周期校验。测试窗口使用内存整帧绘制，避免抗锯齿叠加及绘制中途的画面干扰。
- [完整便携包桌面 14 组](test-results/desktop-temporal-full-package.json)通过，在上述检查之外，实际从界面选择生成的实验模型，在 GTX1650 上完成 CUDA WGC 识别并断言使用压缩图。该包的独立捕获计时另存 `temporal-wgc-full-package.json`，不与 CPU 包的少量手动触发样本混算分位数。
- [最终分发校验](test-results/temporal-package-validation.json)：CPU ZIP 17,002,407 字节，45 个文件/35 个 x64 依赖校验通过；完整 ZIP 1,718,318,976 字节，86 个文件/75 个依赖通过。两包主程序 SHA-256 为 `7fbcd33bda20f2cc14797d5359f5155af497215f58574016b5427339683d8ad5`，与三份桌面报告一致；包内 ONNX Worker 与时序基准二进制一致。NSIS/MSI 未重建，现存安装版不包含这次升级。
- [真实素材标注入口](test-results/temporal-annotation.json)的绘框、编辑、逐帧确认、导出及 SHA-256 导入五项工作流通过。该工作流检查不代表已完成真实游戏准确率测试。

复现完整桌面覆盖可同时设置 `GAMEOCR_TEST_ADAPTIVE=1`、`GAMEOCR_TEST_EFFICIENCY=1`、`GAMEOCR_TEST_TEMPORAL=1`，并用 `GAMEOCR_APP` 指向正式便携包。另设 `GAMEOCR_TEST_COMPACT_DIR` 指向已生成的实验模型目录，可验证模型列表发现/选择及实际 CUDA WGC 推理；需要完整 CUDA 包及兼容硬件。`GAMEOCR_TEMPORAL_DEBUG_FRAMES=1` 只用于诊断，额外写 PNG 会改变送帧间隔，不与常规性能结果混用。

## CUDA 按需下载验证（2026-10-22）

标准包改为首次明确选择 CUDA 并加载模型时准备 NVIDIA 运行库；启动、CPU 与 Auto 均不下载。实现、固定来源和复现步骤见 [运行库说明](cuda-runtime.md)。本轮仍为 Windows 开发机、GTX1650、ORT 1.24.3，使用从 NVIDIA 下载的 CUDA 12.8 组件和 cuDNN 9.8；本地 Worker 本身沿用 CUDA 12.1 编译版本。

- Rust desktop lib 23 项通过（新增 6 项运行库测试），覆盖固定来源、分 Runtime 组件、完整安装/损坏检测、ZIP 路径与缺失文件拒绝、校验/取消/退出、CPU 专用包拒绝及文件锁释放。原断点续传的本机 HTTP 测试也通过。Svelte 检查 0 错误/警告、Vitest 2 项、格式检查及正式 Tauri Release 构建通过。
- [完整官方下载记录](test-results/cuda-runtime-download.json)：独立下载工具使用与桌面相同的 RuntimeManager，从空缓存真实下载全部六个组件，共 1,995,551,251 字节；归档 SHA-256、必需 DLL 和许可证检查通过。记录包含固定来源及提取文件哈希。该轮耗时约 133 秒，仅表示当次网络情况。
- [桌面六组检查](test-results/cuda-runtime-desktop.json)：首次启动/Auto 不建缓存且实际使用 CPU；明确 CUDA 后出现进度，界面取消保留 `.part`；取消后 CPU 可用；重新加载续传并使用缓存中的 DLL；Small/Medium 真实 WGC CUDA OCR；不可达 HTTP/HTTPS 代理下重启，Auto 与显式 CUDA 均离线加载成功。为避免重复下载，最终桌面轮次在取消后复用了上项已下载的五个组件，被取消的 CUDA Runtime 文件仍真实续传；报告明确列出复用项。通过进程模块路径确认实际加载的是本次隔离缓存的 NVIDIA DLL，非包内或开发机 PATH。
- [GGUF CUDA](test-results/cuda-runtime-gguf.json)：仅将已下载 CUDA Runtime/cuBLAS 加入 GPU Worker 路径，真实 Q8_0 Grounding 识别成功，已加载 DLL 的绝对路径均指向下载缓存，正常关闭。
- 最新 CPU 便携包通过 [模型/运行时 12 项](test-results/cuda-runtime-cpu-desktop.json)及 [并行/自适应/小字/增量/时序 13 项](test-results/cuda-runtime-temporal-desktop.json)。包含真实窗口识别、CPU 回退、失败恢复、模型切换、参数/UI、缓存失效、Worker 崩溃恢复与退出清理。应用为单实例，桌面测试依次执行。
- [发行包校验](test-results/cuda-runtime-package-validation.json)：新的 `GameOCR-windows-x64-ondemand-portable.zip` 为 469,949,861 字节，71 个文件/60 个 x64 二进制检查通过；仅允许清单内 NVIDIA 依赖延后提供。CPU ZIP 为 17,118,432 字节，45 个文件/35 个二进制检查通过。两包及桌面报告中的 EXE SHA-256 均为 `42c31b00fd0f952849b18a58b1b7daee0e9ee35e623d305c1c25e0b85ef28a0d`。默认便携包约 470 MB，比上一轮 1.72 GB 完整包小约 73%；首次 ONNX 下载仍约 1.86 GiB，GGUF 约 541 MiB。

完整离线包可继续通过 `-Backend both` 构建；本轮只重建了按需包和 CPU 包，现存旧完整 ZIP 保留上一轮版本。NSIS/MSI 的构建配置和 CI 已更新为按需布局，但安装器未在本轮重新生成，远端 CI 未执行。不将本机验证视为干净 Windows、无驱动机器或 RTX5060Ti 的硬件验收。显卡驱动仍需用户已有；程序不安装驱动或 CUDA Toolkit，不修改系统 PATH。

## GPU 后端也按需下载（2026-10-23）

标准包进一步移除 ORT GPU 主库/provider 和静态链接的 llama.cpp CUDA Worker，仅保留 CPU 后端与小型 ONNX GPU 启动程序。Microsoft ORT 固定为 1.24.3 官方 ZIP，GameOCR Worker 由配套资源脚本生成并在构建时嵌入下载信息；NVIDIA 组件沿用上一轮固定来源。下载、完整性检查、取消续传、修复与离线缓存使用同一套流程，详见 [GPU 组件说明](cuda-runtime.md)。

- Rust desktop lib 26 项通过，新增后端来源/重定向边界、精简包能力判定、ORT 三个库必须配套以及声明文件保留检查；包含原先的下载续传、损坏拒绝及清理测试。Svelte 0 错误/警告，Vitest 2 项通过。
- [GPU 桌面八组](test-results/gpu-backends-desktop.json)：隔离数据目录、开发机 GPU 路径清除；启动和 Auto 不建缓存且使用 CPU；界面取消保留部分下载，CPU 仍可加载；真实完成 Microsoft ORT GPU ZIP 下载并校验；Small/Medium 均完成真实 WGC CUDA 识别。NVIDIA 六个组件复用上一轮已完成的官方缓存，报告明确列出，没有计为本轮网络下载。
- 同一桌面检查通过配套 ZIP 的离线入口安装 llama.cpp GPU Worker，保留 GameOCR、依赖、CUDA EULA 和第三方声明；实际 GGUF Q8_0 WGC 识别通过。进程模块路径证明 ORT 主库/provider、llama.cpp 可执行文件及 NVIDIA DLL 均来自隔离缓存。移除配套 ZIP 后，使用不可达网络代理重启，ONNX Auto/显式 CUDA 及 llama.cpp CUDA 均离线加载成功。
- 最新 CPU 包通过 [模型/运行时十二组](test-results/gpu-backends-cpu-desktop.json)和 [并行/自适应/小字/增量/时序十三组](test-results/gpu-backends-temporal-desktop.json)，包含真实 WGC、损坏恢复、Worker 崩溃及退出清理。所有 GUI 测试串行执行。
- [发行包记录](test-results/gpu-backends-package-validation.json)给出最终字节数、SHA-256 和被测 EXE 哈希。标准 ZIP 约 17.85 MB，比上一轮约 470 MB 缩小约 96.2%；CPU ZIP 约 17.14 MB。两包分别验证 47/45 个归档文件和 36/35 个 x64 文件的原生依赖。标准包没有 GPU DLL 或 llama.cpp GPU Worker；独立 GPU ZIP 约 277 MB，按需准备，未计入基础包。

这次 GGUF 配套资源仅在本地生成并通过离线安装验证，**尚未发布到 GameOCR 的公开 Release，不能声称已验证公网下载**。发布基础包前应上传并公开与 EXE 固定 URL/哈希相符的资源；本地也可放到 `runtime-downloads` 安装。CI 已生成并附带配套资源，Draft Release 必须公开后才可匿名访问。NSIS/MSI 仅更新资源配置，本轮未重建安装器，也未执行远端 CI、干净 Windows 或 RTX5060Ti 验证。
## Translation 验证入口

翻译核心：`cargo test -p gameocr-translation --no-default-features`；桌面配置及生命周期：`cargo test -p gameocr --lib`。真实模型 CLI、固定权重哈希和环境说明见 [translation-runtime.md](translation-runtime.md)。语料为 `tests/translation/corpus.json`，实际 CPU/CUDA 输出在 `docs/test-results/translation-*.json`。

`node scripts/desktop-translation.mjs` 使用独立数据目录、实际 WGC/PP-OCR/Hy-MT2 和桌面 UI。`GAMEOCR_APP` 指定标准便携包，`GAMEOCR_GPU_CACHE` 可指定以前从官方源获取并有完整收据的缓存；Worker 配套 ZIP 来自本地 `artifacts/gpu-backends.json`。`GAMEOCR_TEST_DOWNLOAD=1` 加测真实官方下载取消及 Range 尾段续传，网络受限时配置代理。测试没有 mock 推理耗时，与其他 GUI 测试串行执行。截图、失败详情和中间日志在 `artifacts/validation/translation-*`；只有完整通过才生成 `docs/test-results/translation-desktop.json`。

2026-10-27 翻译验收：60 项 Rust 测试（核心 24、翻译 9、桌面 27）和 2 项前端测试通过；真实翻译桌面检查 16 项通过、显存不足导致的 GPU 组合明确跳过；最终 CPU 包的原 OCR 桌面回归 12 项通过。原生 CPU/CUDA 各 14 条语料、低温度对照、结构合批及输出截断均有原始报告。NSIS/MSI 本轮已重新构建，更新了上方旧 GPU 精简阶段“未重建安装器”的状态。官方下载仍受代理连接失败影响，不计为通过；详见 [交付记录](translation-delivery.md)。

原生测试可运行 `node scripts/test-translation.mjs`，须在构建完成后串行执行。Windows 显存与驱动、CPU 和 OS 见 `test-results/translation-environment.json`；干净虚拟机、真实游戏负载和其他显卡仍未验证。

