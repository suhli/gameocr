# 测试记录

多 Runtime 验证日期：2026-10-14。结果来自真实 ONNX / GGUF 文件和原生 Worker；单元测试中的合成概率图只验证算法，不作为真实 OCR 证据。完整文本、坐标、置信度和分阶段耗时保存在 [test-results](test-results)。

## 环境

Windows 11 x64 开发机，GTX1650 4 GB，NVIDIA 591.86，CUDA12.1，cuDNN9.8.0.87，ORT1.24.3，VS2022/MSVC14.41，Rust1.97.1，Node24.21.0，系统 WebView2。本机 llama.cpp CUDA 使用明确的旧工具链兼容开关及 SM75/80/86/89；CI 配置为 CUDA12.8.1、SM120，但尚未在远端执行。以下耗时有同时编译等背景负载，不是受控性能基准。

## 自动化与真实推理

- Svelte / TypeScript：0 errors、0 warnings；Vitest 2 项坐标映射测试通过；Vite 生产构建成功。
- `cargo test -p gameocr-core --no-default-features --locked`：13 项通过，覆盖 Factory 两类构造分派与扩展注册、设备候选、能力和参数验证、Manifest 迁移、旧历史 JSON、Grounding、变化检测和有界帧槽。
- `cargo test -p gameocr-onnx --locked`：3 项通过，覆盖 CTC blank/重复/混排、DB 空结果及缩放坐标还原、透视裁剪与纵向旋转。
- `cargo test -p gameocr --lib --locked`：12 项通过，覆盖多文件完整性/等长损坏/共享文件删除、导入回滚、真实 loopback HTTP 断点续传、原子配置、窗口身份、16 MiB 协议边界与截断响应拒绝。
- `scripts/test-onnx.mjs`：两规格各 4 张图，CPU 8 例 + CUDA 8 例均通过；还验证错误 JSON、未加载识别、错误 Runtime 拒绝、卸载、重载与进程正常关闭。CUDA Ready 之前检测、识别网络均通过实际 CUDA kernel 执行验证。
- `scripts/test-runtime-switch.mjs`：真实 CPU ONNX small → GGUF Q8_0 → ONNX medium，检查身份、坐标、GGUF 无伪造置信度和每个旧 Worker 退出，三次均通过。
- `scripts/desktop-runtimes.mjs`：真实 Tauri/WebView2 的 11 项检查通过，覆盖真实 CUDA 初始化失败后的 CPU 回退、显式 CUDA 错误、错误后恢复、加载状态参数锁、模型切换、能力驱动设置、两规格 WGC OCR 与匹配预览/JSON、ONNX 三文件导入、等长字典损坏和删除入口、错误网络回滚全部副本。

证据：[CPU 8 例](test-results/onnx-cpu-e2e.json)、[CUDA 8 例](test-results/onnx-cuda-e2e.json)、[跨 Runtime 切换](test-results/runtime-switch.json)、[桌面 11 项](test-results/desktop-runtimes.json)。桌面截图及 WGC 导出保存在本机 `artifacts/validation/multi-runtime/`。

测试输入为本地生成的字体图片：英文 800×320；中日英混排含简体、繁体、日文；约 12° 旋转文字与 16px 小字；空白图。断言真实输出含 GAME OCR TEST、Health、Quest complete、游戏、繁體中文、こんにちは、ROTATED TEXT、Small game text 等关键词，同时检查 bbox、polygon 和置信度，未硬编码替换实际识别结果。

2026-10-14 16:52–16:53 一轮的 totalMs（不含模型加载）：

- small CPU：英文 164ms、混排 307ms、旋转/小字 166ms、空白 127ms。
- medium CPU：英文 590ms、混排 942ms、旋转/小字 637ms、空白 477ms。
- small CUDA：英文 507ms、混排 479ms、旋转/小字 432ms、空白 224ms。
- medium CUDA：英文 1247ms、混排 1591ms、旋转/小字 1442ms、空白 861ms。

GTX1650 上这批小图 CPU 更快。为解决该卡动态识别宽度的 cuDNN 不支持错误，目前使用默认 fallback 卷积算法；没有以理论 GPU 能力替代实测速度。small 与 medium 的混排均产生 4 个真实文字块；空白均返回 0 块。两种模型的完整字典有 18,708 项，不是测试字符子集。

GGUF CPU Plain 和 Grounding 已重新执行；v2 Worker 切换测试保留原始输出/解析行为，已有量化目录和本地导入流程继续可用。[CPU Plain](test-results/gguf-cpu-plain.json)、[CPU Grounding 多对象输出](test-results/gguf-cpu-grounding.jsonl)。后者是 CLI `--repeat` 的多段 pretty JSON，扩展名历史沿用，不是每行一对象的严格 JSONL。

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

桌面脚本自行启动 Vite（先等待可访问）和专用数据目录的测试应用，创建自有 Win32 文本窗口，不操作现有游戏或日常模型目录。Vite 排除 `.cache`、模型及构建/导出目录，避免监视正在复制的 ONNX 临时文件触发 EBUSY。指定 `GAMEOCR_APP` 为便携 EXE 时不启动 Vite。需要可交互桌面，headless CI 不伪造此测试。

## 分发验收

四套 Release Worker 已实际编译，ORT CPU/GPU 和 cuDNN 归档 SHA-256 已核对。原生依赖检查使用 PE 普通及延迟导入表，只在包内目录和 Windows System32 解析；检查 UI 不链接推理库、CPU 不依赖 CUDA。

最终 NSIS/MSI/ZIP 的构建、哈希与便携运行结果见交付时追加的记录。仅生成安装文件不代表已经在干净 Windows 上安装/卸载。

## 原 GGUF 基线记录（2026-10-12 / 13）

旧版已真实执行 WGC、CUDA Grounding、预览/坐标联动、JSON/PNG 导出、变化去重、暂停/恢复、窗口缩放、最小化报错、推理中停止丢弃旧结果、窗口销毁、卸载和正常退出。2026-10-13 的 Release 长测实际完成约 1809 秒 / 176 次采样，历史上限 5；主进程私有内存约 66–70 MiB，worker 约 2439–2991 MiB。证据为本机 `artifacts/validation/desktop-release/{report,summary,soak-progress}.json`。

这些是重构前 GGUF 基线，不能当作新版 ONNX 30 分钟内存测试。更早约 700 秒中断的一轮仍按中断处理，不用它替代完整长测。原 README 中待完成的旧记录已根据现存报告纠正。

## 未验证和已知限制

- RTX5060Ti / SM120、其他显卡，以及 CUDA12.8.1 配置的远端 GitHub Actions；目前硬件验证仅 GTX1650。
- 干净 Windows11、完全无 NVIDIA 驱动机器、安装版系统安装/卸载、签名。PATH 隔离和 CUDA_VISIBLE_DEVICES=-1 只能验证本机子进程路径及故障回退，不能冒称干净机器通过。
- 本轮未重新验证真实 Android / 模拟器 ADB 捕获与断开重连；原测试中只枚举过 ADB，当时设备为空。WGC/ADB 实现未在此重构中替换。
- DirectX/OpenGL/Vulkan 游戏、受保护或反作弊画面、HDR、多显示器不同 DPI；程序不绕过保护。
- Q4_K_M、Q5_K_M、F16 的完整质量/性能对比；目录元数据保留，真实 GGUF 回归采用 Q8_0。
- ONNX 长时压力和广泛准确率评估、任意角度/弯曲文本、竖排中日文、其他 ONNX/GGUF 架构。现 Adapter 只验证指定官方网络和完整字典。
- 模型列表每次扫描文件 SHA-256，大型 GGUF 或慢盘有等待时间；后台线程不会阻塞 UI 事件循环。
- 大型 cuDNN/CUDA 运行库仍占较多发行空间，已跨 Runtime 共享一份；CPU Runtime 可独立构建/拷贝，正式三种组合包均含 CPU/CUDA。

停止 OCR 立即取消结果发布，进入原生库的调用会完成或在 180 秒超时后终止。UI 显存遥测不可用；引擎 totalMs 不含捕获、传输前 PNG 编码、结果 JPEG 和 UI 绘制。
