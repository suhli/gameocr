# GameOCR

Windows 11 x64 本地窗口 OCR 工具，保留 Tauri 2、Svelte 5 和 Rust。Windows Graphics Capture / ADB 共用捕获与调度流程；独立 Worker 通过 Factory + Adapter 接入 llama.cpp（LightOnOCR-3 GGUF）与 ONNX Runtime（PP-OCRv6 small / medium）。两种 Runtime 均支持真实 CPU / CUDA 推理，保留 Grounding、模型下载与导入、历史、坐标联动和导出。当前没有翻译和游戏内覆盖层。

## 使用

1. 解压 `GameOCR-windows-x64-portable.zip`，运行 `GameOCR.exe`；也可使用 NSIS / MSI 安装版。仅使用 CPU 时可选体积更小的 `GameOCR-windows-x64-cpu-portable.zip`，同时支持 GGUF 和 ONNX，不附带 NVIDIA 运行库。
2. 在「模型管理」下载 PP-OCRv6 small / medium，或 LightOnOCR Q8_0。也可导入 GGUF + mmproj，或一组 ONNX 检测模型、识别模型与完整字典。国内下载受限时可在设置里选择 HF-Mirror；完整性仍由固定 SHA-256 校验。
3. 在左侧选择具体窗口，点击「开始捕获」，加载模型，然后点击「单次识别」或「连续」。自动设备选择优先 CUDA，失败时给出原因并尝试 CPU。
4. 「实时」显示当前画面；「识别帧」始终显示与 OCR 结果对应的截图。点击文字块或边界框可相互定位；可查看原图、复制文字、导出 JSON 或 PNG。
5. 模型卸载后可更改推理参数：GGUF 显示上下文、输出预算、温度等；ONNX 显示检测阈值、框阈值、Unclip、识别阈值、Batch Size 和检测缩放。停止捕获后可更改捕获频率、窗口/ADB 来源。

默认捕获 10 FPS、OCR 间隔 1000 ms、变化阈值 0.03。单次识别会强制识别当前帧，连续识别会跳过没有显著变化的画面。推理时只保留一个最新待处理帧，默认保留最近 30 条结果。

暂停后重复识别同一帧会替换该帧的旧结果。历史保留与结果对应的压缩预览；完整原始截图只保留最新识别帧，历史中的旧帧仍可导出 OCR JSON。

Windows 11 便携版直接使用系统现有 WebView2：不执行预安装检查、下载或安装流程，也不携带浏览器。移除了 WebView2 的精简系统不受支持。运行程序不需要 Rust、Python、Visual Studio 或 CUDA Toolkit。CPU Worker 不依赖 CUDA DLL。正式构建默认 CUDA 12.8.1，llama.cpp 编译目标为 SM 75 / 80 / 86 / 89 / 120；ONNX 使用 CUDA 12 版 ORT 与 cuDNN 9.8。RTX 5060 Ti / SM120 尚无硬件实测。本机测试包使用 CUDA 12.1、SM75/80/86/89，仅在 GTX 1650 上验证，不能当作 RTX 5060 Ti 验收包。

设置与模型默认保存于 `%LOCALAPPDATA%\GameOCR`。模型不随发行包附带。退出时清除本次会话的临时截图，历史仅保留在内存；导出文件由用户选择保存位置。`GAMEOCR_DATA_DIR` 可将数据目录设为绝对路径，主要用于隔离测试。

### PP-OCRv6

在「模型管理」筛选 ONNX，下载 small 或 medium，加载后选择「文字 + 原生坐标」。PP-OCRv6 的 Plain 模式也保留原生四点框；它不生成 Prompt、Token 或虚构 rawOutput。文字列表显示真实 CTC 置信度，JSON 同时包含 polygon 与原图像素 bbox。small 检测/识别权重合计约 31 MB，medium 约 139 MB，二者共用约 75 KB 的完整字典。

本地导入时先选择规格，再依次选择 detector ONNX、recognizer ONNX、`ppocrv6_dict.txt`。导入会复制到当前模型目录、校验所有文件，并通过 CPU Worker 实际运行两个网络后才发布 Manifest。任一步失败会回滚本次副本。目录可在设置里修改；已有 GGUF Manifest 自动原地迁移到 v2，不重命名或重新下载权重。支持 ONNX 文件格式不代表支持任意 ONNX 网络，其他模型仍需专用 Adapter。

### PP-OCRv6 可选并行

先卸载模型，在 ONNX 设置中开启「启用多任务并行」，配置设备与实例数，再重新加载。关闭开关即可恢复原单 Worker 路径；GGUF 始终使用原路径。旧设置文件自动补充 `enableParallelInference=false`、`enableCpuWorkers=false`、`gpuWorkerCount=1`、`cpuWorkerCount=1`。

- CPU：只使用 CPU 数量，例如 2；不需要打开「CPU 参与混合并行」。
- CUDA：只使用 GPU 数量，例如 2；初始化失败直接报错。
- Auto：GPU=1、勾选 CPU 参与、CPU=1 可混合运行；GPU=2 / CPU=2 可启动四实例，实际数量受资源检查影响。CUDA 全部不可用时保留 CPU 回退，并说明原因。

GPU 最多 2 个实例，共用同一张显卡；CPU 最多 4 个且不超过逻辑核心数与配置的线程预算。多个 CPU Worker 分摊线程，模型常驻；调整数量前需卸载模型。GPU 加载前会检查可用显存，实际 OOM 仍会报告失败。切片按图像大小和内容选择，跨边界文字不确定时整图复验，可能比单 Worker 更慢。更多实例或混合 CPU/GPU 不保证加速。

性能面板下方可展开端到端、传输、切片、排队、执行、合并、失败/重试和每个 Worker 状态。实测方法、六模式数据、准确率及限制见 [并行测试报告](docs/parallel-performance.md)；截图语料采用明确标注的合成画面。

### Android / 模拟器

切换到 Android，设置已有的 `adb.exe` 路径，刷新设备并选择序列号。可输入模拟器提供的 `host:port` 地址连接；没有硬编码端口。设备断开会停止捕获并显示错误，恢复连接后刷新设备并重新开始。ADB 不随发行包携带。WGC 与 ADB 最终都产生同一种 RGB 帧。

## 本地开发

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

脚本会导入 VS 开发环境，使用 Ninja、配置 `LIBCLANG_PATH`，收集 CUDA 和 MSVC app-local DLL。可用 `-Runtime onnx` 或 `-Runtime llamacpp` 单独构建。CPU / CUDA ORT DLL 放在各自 Worker 旁；大型 CUDA/cuDNN DLL 仅在 `workers/cuda-runtime` 存一份，并仅加入 GPU 子进程 PATH。仅针对本机旧 CUDA 12.1 / 较新 MSVC 的验证环境提供 `-LocalCuda121Compatibility -CudaArchitectures '75;80;86;89'`；正式 CI 不使用该开关。

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
$env:GAMEOCR_APP=(Resolve-Path ./artifacts/GameOCR-windows-x64-portable/GameOCR.exe).Path
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
```

GitHub Actions 自动完成同样步骤并生成完整便携 ZIP、独立 CPU ZIP、NSIS、MSI 与各自的 SHA256SUMS。推送 `v*` 标签时创建待审阅的 Draft Release。GPU 推理和交互桌面测试不在普通 hosted runner 中伪装执行。

多 Runtime 的原生库超过单个压缩块容量，因此安装模板使用 NSIS 逐文件 LZMA、MSI 多个嵌入式 CAB；模板基于 lockfile 中的 Tauri CLI 2.12.2。升级 CLI 时按 [安装模板说明](src-tauri/installer/README.md) 同步检查模板。

当前实测、未测试项和已知限制见 [测试报告](docs/testing.md)。普通游戏、模拟器及受保护画面的捕获兼容性取决于目标应用与系统，程序不绕过保护或反作弊。便携包的干净 Windows 11 验证必须与开发机实测分别记录。发行包当前未签名。

## 代码结构

- `crates/gameocr-core`：Engine Trait/Factory、Runtime Registry、Manifest 迁移、统一类型、变化检测、单槽队列、Grounding、LightOnOCR Adapter 与 MTMD。
- `crates/gameocr-onnx`：ORT Session、PP-OCRv6 检测/透视校正/批量识别/CTC、CPU/CUDA Worker 与独立 CLI。
- `src-tauri/src`：窗口身份、WGC/ADB、调度、worker 生命周期、模型下载/导入、配置、IPC。
- `src`：Svelte 页面与组件、统一 IPC client、坐标映射。
- `scripts`：独立后端构建、许可证/运行库收集、便携打包、真实桌面测试。
- `.github/workflows/windows.yml`：自动测试、编译、NSIS/MSI/Portable 与 Draft Release。

架构与取消语义见 [架构说明](docs/architecture.md)。许可证为 MIT，第三方组件及运行库保留各自条款，见 [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)。
