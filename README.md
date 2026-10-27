# GameOCR

Windows 11 x64 本地窗口 OCR 与离线翻译工具，使用 Tauri 2、Svelte 5 和 Rust。Windows Graphics Capture / ADB 共用捕获与调度流程；独立 Worker 接入 llama.cpp（LightOnOCR-3 GGUF）、ONNX Runtime（PP-OCRv6 small / medium）和 Hy-MT2 翻译。OCR 与翻译分别设置 CPU / CUDA / Auto，保留 Grounding、模型管理、历史、坐标联动和原始导出。翻译默认关闭，当前没有游戏内覆盖层或 TTS。

## 使用

1. 解压 `GameOCR-windows-x64-ondemand-portable.zip`，运行 `GameOCR.exe`。标准便携包在首次明确选择 CUDA 并加载模型时下载 GPU 后端和运行库。CPU 专用包不启用 GPU 下载；两种包均包含独立 CPU 翻译 Worker，模型权重另行下载。NSIS / MSI 构建采用相同的精简资源配置。
2. 在「模型管理」下载 PP-OCRv6 small / medium，或 LightOnOCR Q8_0。也可导入 GGUF + mmproj，或一组 ONNX 检测模型、识别模型与完整字典。国内下载受限时可在设置里选择 HF-Mirror；完整性仍由固定 SHA-256 校验。
3. 在左侧选择具体窗口，点击「开始捕获」，加载模型，然后点击「单次识别」或「连续」。自动设备选择优先使用已准备好的 CUDA，否则给出原因并尝试 CPU；Auto 不会触发运行库下载。
4. 「实时」显示当前画面；「识别帧」始终显示与 OCR 结果对应的截图。点击文字块或边界框可相互定位；可查看原图、复制文字、导出 JSON 或 PNG。
5. 模型卸载后可更改推理参数：GGUF 显示上下文、输出预算、温度等；ONNX 显示检测阈值、框阈值、Unclip、识别阈值、Batch Size 和检测缩放。停止捕获后可更改捕获频率、窗口/ADB 来源。

默认捕获 10 FPS、OCR 间隔 1000 ms、变化阈值 0.03。单次识别会强制识别当前帧，连续识别会跳过没有显著变化的画面。推理时只保留一个最新待处理帧，默认保留最近 30 条结果。

暂停后重复识别同一帧会替换该帧的旧结果。历史保留与结果对应的压缩预览；完整原始截图只保留最新识别帧，历史中的旧帧仍可导出 OCR JSON。

Windows 11 便携版直接使用系统现有 WebView2：不执行 WebView2 预安装检查、下载或安装流程，也不携带浏览器。移除了 WebView2 的精简系统不受支持。运行程序不需要 Rust、Python、Visual Studio 或 CUDA Toolkit。CPU Worker 不依赖 CUDA DLL。正式构建默认 CUDA 12.8.1，llama.cpp 编译目标为 SM 75 / 80 / 86 / 89 / 120；ONNX 使用 CUDA 12 版 ORT 与 cuDNN 9.8。RTX 5060 Ti / SM120 尚无硬件实测。本机 Worker 由 CUDA 12.1、SM75/80/86/89 编译，按需下载的运行库为 CUDA 12.8 / cuDNN 9.8；本机验证仅覆盖 GTX 1650，不能当作 RTX 5060 Ti 验收包。

设置与模型默认保存于 `%LOCALAPPDATA%\GameOCR`。模型不随发行包附带。退出时清除本次会话的临时截图，历史仅保留在内存；导出文件由用户选择保存位置。`GAMEOCR_DATA_DIR` 可将数据目录设为绝对路径，主要用于隔离测试。

### CUDA 按需下载

首次启动、CPU 模式和 Auto 均不下载。在设置中明确选择「NVIDIA CUDA」后加载模型，程序从 Microsoft 下载匹配的 ORT GPU 主库及 provider，从 GameOCR Release 下载配套 llama.cpp GPU Worker，从 NVIDIA 下载 CUDA/cuDNN。显示进度，支持取消、续传与校验。本机构建全部缺失时 GGUF 首次约 805 MiB，ONNX 约 2.15 GiB；不含权重，两者共用已准备的 NVIDIA 组件。缓存可离线复用，加载前重新校验。

组件保存于 `%LOCALAPPDATA%\GameOCR\runtimes` 下的 `nvidia` / `backends`，只供 GPU 子进程使用，不修改系统 PATH、不安装 CUDA Toolkit 或显卡驱动。CPU 专用包使用 CUDA 时需换标准包；完整离线包可通过 `-Backend both` 构建。**当前本地 llama.cpp GPU 资源已生成，尚未发布到 GameOCR Release；公网下载需先发布配套资源，也可在 EXE 旁的 `runtime-downloads` 放置配套 ZIP。** 版本、来源和验证方式见 [GPU 组件说明](docs/cuda-runtime.md)。

### PP-OCRv6

在「模型管理」筛选 ONNX，下载 small 或 medium，加载后选择「文字 + 原生坐标」。PP-OCRv6 的 Plain 模式也保留原生四点框；它不生成 Prompt、Token 或虚构 rawOutput。文字列表显示真实 CTC 置信度，JSON 同时包含 polygon 与原图像素 bbox。small 检测/识别权重合计约 31 MB，medium 约 139 MB，二者共用约 75 KB 的完整字典。

本地导入时先选择规格，再依次选择 detector ONNX、recognizer ONNX、`ppocrv6_dict.txt`。导入会复制到当前模型目录、校验所有文件，并通过 CPU Worker 实际运行两个网络后才发布 Manifest。任一步失败会回滚本次副本。目录可在设置里修改；已有 GGUF Manifest 自动原地迁移到 v2，不重命名或重新下载权重。支持 ONNX 文件格式不代表支持任意 ONNX 网络，其他模型仍需专用 Adapter。

### PP-OCRv6 执行策略

先卸载模型，在 ONNX 设置的「OCR 执行策略」中选择模式，再重新加载：

- **单 Worker**（默认）：沿用 CPU / CUDA / Auto 的原执行路径。
- **手动并行**：配置 CPU / GPU 实例数，使用原切片与合并流程。
- **自适应优化**：用传统图像特征匹配本机历史，在 CPU×1 与 CPU×2 之间选择；证据不足时从完整单 Worker 开始。CPU / Auto 可用，显式 CUDA 需改为 CPU 或选择其他模式。GPU 候选暂未启用。

旧配置开启并行时迁移为 Manual，否则为 Single，升级不会自动开启 Adaptive。GGUF 始终使用原执行路径。自适应默认允许在线学习与低频探索，CPU 上限 2、GPU 上限 0；可关闭探索、关闭学习（只读历史），或查看统计、清空历史。学习档案位于数据目录的 `adaptive-profiles.json`，只保存特征与聚合统计，不保存截图或 OCR 文字。实际开销、收益及限制见 [自适应调度报告](docs/adaptive-scheduling.md)。

### 高分辨率、小字与连续画面

ONNX 设置还提供指定区域、完整小字分块、疑似漏字区域补检、增量识别和按文字宽度分批，均默认关闭。仅需字幕／HUD 时可限定区域；连续画面可开启增量识别，复用像素完全相同的区域；密集小字可选择有预算的补检或更完整、耗时更高的分块。局部、小字、补检和增量模式使用单 Worker 与完整线程预算，避免重复分块；关闭后恢复原执行策略。

输入缓冲区复用、PPM 传输及 CTC 扫描优化自动生效。FP16／INT8 已提供离线转换与评估工具，但本机 GTX1650／5600G 的实验未提速，默认模型继续使用 FP32。具体测量、适用范围和复现方式见 [六项效率优化](docs/ocr-efficiency.md)。

手动并行设备设置：

- CPU：只使用 CPU 数量，例如 2；不需要打开「CPU 参与混合并行」。
- CUDA：只使用 GPU 数量，例如 2；初始化失败直接报错。
- Auto：GPU=1、勾选 CPU 参与、CPU=1 可混合运行；GPU=2 / CPU=2 可启动四实例，实际数量受资源检查影响。CUDA 全部不可用时保留 CPU 回退，并说明原因。

GPU 最多 2 个实例，共用同一张显卡；CPU 最多 4 个且不超过逻辑核心数与配置的线程预算。多个 CPU Worker 分摊线程，模型常驻；调整数量前需卸载模型。GPU 加载前会检查可用显存，实际 OOM 仍会报告失败。切片按图像大小和内容选择，跨边界文字不确定时整图复验，可能比单 Worker 更慢。更多实例或混合 CPU/GPU 不保证加速。

性能面板下方可展开端到端、传输、切片、排队、执行、合并、失败/重试和每个 Worker 状态。实测方法、六模式数据、准确率及限制见 [并行测试报告](docs/parallel-performance.md)；截图语料采用明确标注的合成画面。

### Android / 模拟器

切换到 Android，设置已有的 `adb.exe` 路径，刷新设备并选择序列号。可输入模拟器提供的 `host:port` 地址连接；没有硬编码端口。设备断开会停止捕获并显示错误，恢复连接后刷新设备并重新开始。ADB 不随发行包携带。WGC 与 ADB 最终都产生同一种 RGB 帧。

## 本地开发

本地开发沿用 `main`。翻译核心、运行时与桌面集成是增量模块，不改写现有 OCR Engine。

### 本地翻译

1. 点击顶部「翻译」，在「翻译模型」下载或导入官方 Hy-MT2-1.8B Q4_K_M（约 1.06 GiB）。模型管理也有独立「OCR 模型 / 翻译模型」分类。
2. 保存翻译设备、源/目标语言和稳定等待设置，再加载翻译模型；启用「OCR 完成后自动翻译」。默认目标为简体中文。
3. 加载原 OCR 模型并捕获窗口，启动单次或连续 OCR。结果面板可选原文、译文、两者，并独立复制/导出译文。
4. 逐字对白可调整 Fast/Balanced/Accurate 的等待窗口。重复文字复用有界缓存；暂停和切换目标会使旧任务失效。
5. 可建立游戏 Profile、编辑/导入/导出术语 JSON，并手动绑定稳定进程名称。上下文默认关闭，开启后需手动确认对白才加入有限上下文。

显式 CUDA 失败不会悄悄回退。Auto 只使用已经准备好的 GPU 组件，可回退 CPU。4GB 显卡同时运行游戏可能无法通过准入；CPU 始终可选。Balanced 至少保留 2GiB 显存，其他策略的预留可调整，预估不等于真实占用。GPU 下载包必须与应用嵌入的版本一致，发布前需将两个 Worker 组件公开上传到配套 Release。

详见 [翻译架构](docs/translation-architecture.md)、[Runtime 与模型校验](docs/translation-runtime.md)、[性能](docs/translation-performance.md)、[质量与已知边界](docs/translation-quality.md)。其他 GGUF/ONNX 翻译模型尚未注册；首批本地导入仅接受已验证的 Hy-MT2 Q4_K_M。

当前构建与验收汇总见 [翻译交付记录](docs/translation-delivery.md)：真实 CPU/CUDA 原生推理通过，桌面 CPU 管线、缓存与取消恢复通过；桌面双 GPU 组合因显存不足跳过，当前代理下的模型下载尚未完成真实续传验收。

构建机需要 Windows、Node.js 24、Rust stable MSVC 工具链、VS 2022 C++ 桌面开发、Windows SDK、CMake、Ninja、LLVM/libclang。CUDA 发布构建需要 CUDA 12.8.1 和 cuDNN 9.8.0.87。关键依赖固定 `llama-cpp-2 0.1.159`、`windows-capture 2.0.1`、`ort 2.0.0-rc.12`（API 24 / 动态加载）、ORT 1.24.3、`imageproc 0.25.0`；完整版本见 lockfile。ort 最低 Rust 1.88，项目以 Rust 1.97.1 实测。

```powershell
npm ci
# CPU 开发可先只构建 CPU worker；自动模式会回退到它
./scripts/build-workers.ps1 -Backend cpu -Profile release
node scripts/collect-licenses.mjs
npm run desktop
```

`npm run desktop` 启动 Tauri dev 和 Vite。仅 `npm run dev` 是浏览器界面预览，不能进行本机捕获或 OCR。首次编译原生依赖较慢。

```powershell
# 首次取得固定版本 cuDNN；校验 SHA-256 并设置本进程 CUDNN_PATH
./scripts/fetch-cudnn.ps1
# 同时构建两种 Runtime 的 CPU + CUDA，共四套 Worker
./scripts/build-workers.ps1 -Backend both -Profile release
```

脚本会导入 VS 开发环境，使用 Ninja、配置 `LIBCLANG_PATH`，收集 CUDA 和 MSVC app-local DLL。可用 `-Runtime onnx` 或 `-Runtime llamacpp` 单独构建。开发目录与完整离线包保留四套 Worker，CUDA/cuDNN 在 `workers/cuda-runtime` 共享一份；标准包只保留 CPU 后端与小型 ONNX GPU 启动程序，其余 GPU 组件按需下载。仅针对本机旧 CUDA 12.1 / 较新 MSVC 的验证环境提供 `-LocalCuda121Compatibility -CudaArchitectures '75;80;86;89'`；正式 CI 不使用该开关。

## 独立模型验证
 
```powershell
cargo run -p gameocr-core --bin model-test --release -- `
  --model ./models/LightOnOCR-3-0.8B-Q8_0.gguf `
  --mmproj ./models/LightOnOCR-3-0.8B-mmproj-f16.gguf `
  --image ./tests/fixtures/english.png --mode grounding --device cpu --repeat 2

# 已构建的 CUDA CLI 不需要再次编译
$env:PATH=(Resolve-Path ./src-tauri/resources/workers/cuda-runtime).Path+';'+$env:PATH
./target/workers-cuda/release/model-test.exe `
  --model ./models/LightOnOCR-3-0.8B-Q8_0.gguf `
  --mmproj ./models/LightOnOCR-3-0.8B-mmproj-f16.gguf `
  --image ./tests/fixtures/english.png --mode grounding --device cuda --repeat 2
```

输出包括实际计算设备、原始生成文本、解析结果及分段耗时。`--repeat` 输出多个独立 JSON 对象。普通 OCR 使用 `--mode plain`。调试时可减小 `--max-tokens` 和 `--max-image-edge`。模型和视觉编码器只加载一次，每张图创建独立推理上下文，避免前一张图污染 KV / recurrent state。

PP-OCRv6 不需要 Python，使用 PowerShell 7 下载已固定 revision、大小与 SHA-256 的官方 ONNX 文件，然后运行 CLI：

```powershell
./scripts/fetch-onnx-models.ps1
$env:ORT_DYLIB_PATH=(Resolve-Path ./src-tauri/resources/workers/onnx/cpu/onnxruntime.dll).Path
./target/onnx-cpu/release/onnx-test.exe --manifest .cache/test-models/ppocrv6-small.json `
  --image tests/fixtures/mixed-cjk.png --device cpu --repeat 2 --output artifacts/ppocr-small.json
$env:GAMEOCR_ONNX_WORKER=(Resolve-Path ./src-tauri/resources/workers/onnx/cpu/onnx-worker.exe).Path
node scripts/test-onnx.mjs
```

CLI 输出包含 Model ID、Runtime、实际设备、预处理/检测/识别/后处理及总耗时、全部文本、四点框和置信度。将 Manifest 换成 `ppocrv6-medium.json` 即可验证 medium。GPU CLI 使用 `target/onnx-cuda/release/onnx-test.exe --device cuda`，将 ORT_DYLIB_PATH 指向 `workers/onnx/cuda/onnxruntime.dll`，PATH 加入 `workers/cuda-runtime`。CLI 必须明确选择 cpu/cuda，桌面 Auto 由 WorkerManager 执行跨进程回退。

官方模型与 Prompt 的核对、真实推理结果见 [模型兼容性](docs/model-compatibility.md)。Q8 已在 CPU 与 GTX 1650 CUDA 上完成真实推理；目录内其他量化版本是否实际运行过以测试报告为准。

## 测试与发布

```powershell
npm run check
npm test
npm run build
cargo fmt --all -- --check
cargo test -p gameocr-core --no-default-features --locked
cargo test -p gameocr-onnx --locked
cargo test -p gameocr --lib --locked
```

真实桌面测试使用应用自身的 WebView2 调试接口，直接调用真实 Tauri IPC，捕获自建 Win32 测试窗口；不模拟 OCR。需要已下载模型与 worker、运行中的 Vite，以及可交互 Windows 桌面：

```powershell
cargo build -p gameocr --features test-fixture --bins
# 另一个终端运行 npm run dev
$env:GAMEOCR_SOAK_SECONDS='1800'
node scripts/desktop-smoke.mjs
```

验证已经构建的便携版时不需要 Vite：先构建上述测试窗口，然后指定实际 EXE。测试使用独立数据目录，不修改日常使用的设置；需要约 1 GB 空间导入验证模型。

```powershell
$env:GAMEOCR_APP=(Resolve-Path ./artifacts/GameOCR-windows-x64-ondemand-portable/GameOCR.exe).Path
$env:GAMEOCR_TEST_OUTPUT=(Join-Path $PWD 'artifacts/validation/portable-final')
$env:GAMEOCR_SOAK_SECONDS='1800'
node scripts/desktop-smoke.mjs
# 前一程序完全退出后，检查自动 CPU 回退、损坏模型恢复与退出清理
$env:GAMEOCR_MODEL_DIR=(Join-Path $env:GAMEOCR_TEST_OUTPUT 'user-data/models')
node scripts/desktop-extra.mjs
```

构建三个发行包：

```powershell
./scripts/build-workers.ps1 -Backend both -Profile release
node scripts/collect-licenses.mjs
# 生成独立 GPU ZIP 和主程序内嵌清单；发布时指定实际标签 -ReleaseTag vYOUR_VERSION
./scripts/package-gpu-backends.ps1
npm run tauri -- build --bundles nsis,msi
New-Item -ItemType Directory -Force artifacts | Out-Null
Copy-Item target/release/bundle/nsis/*.exe artifacts/GameOCR-windows-x64-setup.exe
Copy-Item target/release/bundle/msi/*.msi artifacts/GameOCR-windows-x64.msi
# 便携配置与安装配置独立；不运行安装器来制作 ZIP
npm run tauri -- build --no-bundle --config src-tauri/tauri.portable.conf.json
./scripts/package-portable.ps1
./scripts/verify-package.ps1
# 可选：单独分发两个 Runtime 的 CPU Worker
./scripts/package-portable.ps1 -Backend cpu
./scripts/verify-package.ps1 -Backend cpu
# 可选：附带全部 NVIDIA 运行库，供离线使用
./scripts/package-portable.ps1 -Backend both
./scripts/verify-package.ps1 -Backend both -PortableOnly
```

只更新便携版时，先在当前终端设置 `$env:GAMEOCR_GPU_MANIFEST=(Resolve-Path artifacts/gpu-backends.json).Path`，使用上面的 Tauri `build --no-bundle --config src-tauri/tauri.portable.conf.json`，然后打包并运行 `./scripts/verify-package.ps1 -PortableOnly`。发布主程序必须由 Tauri CLI 构建，它会启用加载内嵌前端所需的 `custom-protocol`；之后不要用普通 `cargo build --release --bins` 覆盖 `target/release/gameocr.exe`。单独构建基准工具请指定 `--bin executor-bench`。验证便携包时设置 `GAMEOCR_APP`，运行 `node scripts/desktop-parallel.mjs`，确认没有 Vite 也能打开应用并完成实际识别。

GitHub Actions 生成按需便携 ZIP、独立 CPU ZIP、NSIS、MSI、独立 llama.cpp GPU ZIP、内嵌下载清单副本及 SHA256SUMS。标准包不包含 ORT GPU DLL、llama.cpp GPU Worker 或 NVIDIA 库。推送 `v*` 标签创建 Draft Release；必须发布其中的配套 GPU 资源，程序才能匿名下载。GPU 推理和交互桌面测试不在普通 hosted runner 中伪装执行。

安装模板保留 NSIS 逐文件 LZMA、MSI 多个嵌入式 CAB，以支持包含大型原生库的布局；标准按需配置已不携带这些 GPU 文件。模板基于 lockfile 中的 Tauri CLI 2.12.2，升级 CLI 时按 [安装模板说明](src-tauri/installer/README.md) 同步检查。

当前实测、未测试项和已知限制见 [测试报告](docs/testing.md)。普通游戏、模拟器及受保护画面的捕获兼容性取决于目标应用与系统，程序不绕过保护或反作弊。便携包的干净 Windows 11 验证必须与开发机实测分别记录。发行包当前未签名。

## 代码结构

- `crates/gameocr-core`：Engine Trait/Factory、Runtime Registry、Manifest 迁移、统一类型、变化检测、单槽队列、Grounding、LightOnOCR Adapter 与 MTMD。
- `crates/gameocr-onnx`：ORT Session、PP-OCRv6 检测/透视校正/批量识别/CTC、CPU/CUDA Worker 与独立 CLI。
- `src-tauri/src`：窗口身份、WGC/ADB、调度、worker 生命周期、模型下载/导入、配置、IPC。
- `src`：Svelte 页面与组件、统一 IPC client、坐标映射。
- `scripts`：独立后端构建、许可证/运行库收集、便携打包、真实桌面测试。
- `.github/workflows/windows.yml`：自动测试、编译、NSIS/MSI/Portable 与 Draft Release。

架构与取消语义见 [架构说明](docs/architecture.md)。许可证为 MIT，第三方组件及运行库保留各自条款，见 [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)。

## 实时时序识别（实验性）

新增逐阶段延迟统计、时序文字跟踪、独立 Rec-only、可选共享内存和 CUDA 概率压缩。时序与实验加速开关默认关闭，统计始终可用；在设置中可启用相应策略，Manual/Adaptive 时序会限制为单 Worker。参见 [时序使用与标注](docs/temporal-ocr.md) 和 [分阶段实测、兼容性及限制](docs/latency-optimization.md)。RTX5060Ti 尚未实测；当前 GTX1650 不支持 TensorRT RTX。
