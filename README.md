# GameOCR

Windows 11 x64 本地窗口 OCR 工具。Tauri 2、Svelte 5 和 Rust；使用 Windows Graphics Capture 截图，使用 `llama-cpp-2::mtmd` 运行 LightOnOCR-3-0.8B。包含真实 CPU / CUDA 推理、Grounding、模型管理、ADB 捕获、历史记录与性能监控。第一阶段没有翻译和游戏内覆盖层。

## 使用

1. 解压 `GameOCR-windows-x64-portable.zip`，运行 `GameOCR.exe`；也可使用 NSIS / MSI 安装版。
2. 在「模型管理」下载 Q8_0，或依次导入主模型 GGUF 与匹配的 mmproj GGUF。国内下载受限时可在设置里选择 HF-Mirror；完整性仍由固定 SHA-256 校验。
3. 在左侧选择具体窗口，点击「开始捕获」，加载模型，然后点击「单次识别」或「连续」。自动设备选择优先 CUDA，失败时给出原因并尝试 CPU。
4. 「实时」显示当前画面；「识别帧」始终显示与 OCR 结果对应的截图。点击文字块或边界框可相互定位；可查看原图、复制文字、导出 JSON 或 PNG。
5. 模型卸载后可更改推理设备、上下文和输出预算。停止捕获后可更改捕获频率、窗口/ADB 来源。

默认捕获 10 FPS、OCR 间隔 1000 ms、变化阈值 0.03。单次识别会强制识别当前帧，连续识别会跳过没有显著变化的画面。推理时只保留一个最新待处理帧，默认保留最近 30 条结果。

暂停后重复识别同一帧会替换该帧的旧结果。历史保留与结果对应的压缩预览；完整原始截图只保留最新识别帧，历史中的旧帧仍可导出 OCR JSON。

Windows 11 便携版直接使用系统现有 WebView2：不执行预安装检查、下载或安装流程，也不携带浏览器。移除了 WebView2 的精简系统不受支持。运行程序不需要 Rust、Python、Visual Studio 或 CUDA Toolkit。CUDA 需要兼容的 NVIDIA 驱动；CPU 后端独立分发。当前 CUDA 编译目标为 SM 75 / 80 / 86 / 89，其他 GPU 可选择 CPU，尚未逐型号验证。

设置与模型默认保存于 `%LOCALAPPDATA%\GameOCR`。模型不随发行包附带。退出时清除本次会话的临时截图，历史仅保留在内存；导出文件由用户选择保存位置。`GAMEOCR_DATA_DIR` 可将数据目录设为绝对路径，主要用于隔离测试。

### Android / 模拟器

切换到 Android，设置已有的 `adb.exe` 路径，刷新设备并选择序列号。可输入模拟器提供的 `host:port` 地址连接；没有硬编码端口。设备断开会停止捕获并显示错误，恢复连接后刷新设备并重新开始。ADB 不随发行包携带。WGC 与 ADB 最终都产生同一种 RGB 帧。

## 本地开发

构建机需要 Windows、Node.js 24、Rust stable MSVC 工具链、VS 2022 C++ 桌面开发、Windows SDK、CMake、Ninja、LLVM/libclang。构建 CUDA 后端还需要 CUDA 12.x Toolkit；CI 固定 12.8.1。提交的两个 lockfile 锁定实际依赖，关键原生库固定 `llama-cpp-2 0.1.159`、`windows-capture 2.0.1`。

```powershell
npm ci
# CPU 开发可先只构建 CPU worker；自动模式会回退到它
./scripts/build-workers.ps1 -Backend cpu -Profile release
node scripts/collect-licenses.mjs
npm run desktop
```

`npm run desktop` 启动 Tauri dev 和 Vite。仅 `npm run dev` 是浏览器界面预览，不能进行本机捕获或 OCR。首次编译原生依赖较慢。

```powershell
# 同时构建 CPU + CUDA，两套运行程序彼此隔离
./scripts/build-workers.ps1 -Backend both -Profile release
```

脚本会导入 VS 开发环境，使用 Ninja、配置 `LIBCLANG_PATH`，收集 CUDA 和 MSVC app-local DLL。仅针对本机旧 CUDA 12.1 / 较新 MSVC 的验证环境提供 `-LocalCuda121Compatibility` 开关；正式 CI 不使用该兼容开关。

## 独立模型验证

```powershell
cargo run -p gameocr-core --bin model-test --release -- `
  --model ./models/LightOnOCR-3-0.8B-Q8_0.gguf `
  --mmproj ./models/LightOnOCR-3-0.8B-mmproj-f16.gguf `
  --image ./tests/fixtures/english.png --mode grounding --device cpu --repeat 2

# 已构建的 CUDA CLI 不需要再次编译
$env:PATH=(Resolve-Path ./src-tauri/resources/workers/cuda).Path+';'+$env:PATH
./target/workers-cuda/release/model-test.exe `
  --model ./models/LightOnOCR-3-0.8B-Q8_0.gguf `
  --mmproj ./models/LightOnOCR-3-0.8B-mmproj-f16.gguf `
  --image ./tests/fixtures/english.png --mode grounding --device cuda --repeat 2
```

输出包括实际计算设备、原始生成文本、解析结果及分段耗时。`--repeat` 输出多个独立 JSON 对象。普通 OCR 使用 `--mode plain`。调试时可减小 `--max-tokens` 和 `--max-image-edge`。模型和视觉编码器只加载一次，每张图创建独立推理上下文，避免前一张图污染 KV / recurrent state。

官方模型与 Prompt 的核对、真实推理结果见 [模型兼容性](docs/model-compatibility.md)。Q8 已在 CPU 与 GTX 1650 CUDA 上完成真实推理；目录内其他量化版本是否实际运行过以测试报告为准。

## 测试与发布

```powershell
npm run check
npm test
npm run build
cargo fmt --all -- --check
cargo test -p gameocr-core --no-default-features --locked
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
# 便携配置与安装配置独立；不运行安装器来制作 ZIP
npm run tauri -- build --no-bundle --config src-tauri/tauri.portable.conf.json
./scripts/package-portable.ps1
```

GitHub Actions 自动完成同样步骤并生成 `GameOCR-windows-x64-portable.zip`、`GameOCR-windows-x64-setup.exe`、`GameOCR-windows-x64.msi` 与 SHA256SUMS。推送 `v*` 标签时创建待审阅的 Draft Release。GPU 推理和交互桌面测试不在普通 hosted runner 中伪装执行。

当前实测、未测试项和已知限制见 [测试报告](docs/testing.md)。普通游戏、模拟器及受保护画面的捕获兼容性取决于目标应用与系统，程序不绕过保护或反作弊。便携包的干净 Windows 11 验证必须与开发机实测分别记录。发行包当前未签名。

## 代码结构

- `crates/gameocr-core`：类型、变化检测、单槽队列、Grounding 解析、MTMD 引擎与两种 CLI。
- `src-tauri/src`：窗口身份、WGC/ADB、调度、worker 生命周期、模型下载/导入、配置、IPC。
- `src`：Svelte 页面与组件、统一 IPC client、坐标映射。
- `scripts`：独立后端构建、许可证/运行库收集、便携打包、真实桌面测试。
- `.github/workflows/windows.yml`：自动测试、编译、NSIS/MSI/Portable 与 Draft Release。

架构与取消语义见 [架构说明](docs/architecture.md)。许可证为 MIT，第三方组件及运行库保留各自条款，见 [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)。
