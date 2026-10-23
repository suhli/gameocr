# GPU 后端与 CUDA 运行库按需下载

标准 `ondemand` 便携包只保留 CPU 所需文件和约 1.6 MB 的 ONNX CUDA 启动程序，不携带 ORT GPU DLL、llama.cpp CUDA Worker 或 NVIDIA 运行库。首次启动、CPU 与 Auto 均不下载；明确选择 NVIDIA CUDA 并加载模型时才准备对应组件。CPU 专用包不启用下载，完整离线包仍可用 `-Backend both` 构建。

## 固定来源与版本配套

- ONNX GPU 库从 [Microsoft ONNX Runtime 1.24.3](https://github.com/microsoft/onnxruntime/releases/tag/v1.24.3) 官方 Release 下载。锁定的 GPU ZIP 为 311,219,580 字节、SHA-256 `f2af33fea71f477027961b8027e7496ab01c6b962ae4f596bf8163192b03b326`。同时提取 `onnxruntime.dll`、`onnxruntime_providers_shared.dll` 和 `onnxruntime_providers_cuda.dll`，避免混用 CPU 主库或不同版本的 provider。清单见 [onnx-cuda-runtime.json](../src-tauri/catalog/onnx-cuda-runtime.json)。
- llama.cpp GPU 实现静态链接在 GameOCR 的 `ocr-worker.exe` 中，需从 GameOCR 自己的 Release 下载配套 Worker。通用 llama.cpp 可执行文件不实现本项目 Worker 协议，不能直接替换。打包脚本生成内容哈希命名的 ZIP，并将确切 URL、大小、SHA-256 嵌入主程序。本机这份资源为 277,372,122 字节；不同 GPU 架构的构建体积会变化。
- CUDA/cuDNN 从 NVIDIA 官方获取。版本、大小和哈希固定于 [cuda-runtime.json](../src-tauri/catalog/cuda-runtime.json)，来源为 [CUDA 12.8.1 清单](https://developer.download.nvidia.com/compute/cuda/redist/redistrib_12.8.1.json)及 [cuDNN 9.8.0 清单](https://developer.download.nvidia.com/compute/cudnn/redist/redistrib_9.8.0.json)。两种 Runtime 共用 CUDA Runtime/cuBLAS，ONNX 还需 cuFFT、NVRTC、nvJitLink、cuDNN。

全部缺失时，本机 GGUF GPU 组件约 805 MiB（含 Worker），ONNX 约 2.15 GiB（含 ORT GPU）。这是完整官方归档的下载量，不含模型权重；只提取所需文件和许可证，不安装开发工具。已准备的共享组件不会重复下载。

下载初始地址仅接受固定的 NVIDIA redistributable 或 Microsoft/GameOCR Release 路径；重定向仅允许 NVIDIA `.com`/`.cn`、上述 GitHub 路径及 GitHub 官方 Release CDN。所有下载必须匹配应用内固定哈希。`gpu-downloads.json` 只开启标准包下载能力，不承载可修改的下载 URL。

## 缓存、取消与离线使用

界面显示组件和下载/校验/解压进度，支持取消和 Range 续传；服务端不支持 Range 时安全重下，网络错误最多自动重试三次。归档先验证大小及 SHA-256，再在临时目录中提取。拒绝越界路径、符号链接、重复文件、缺失组件及超限数据。完成后发布组件和文件哈希记录；失败或取消保留 `.part` 和已安装的有效组件，不发布部分安装。

缓存位于 `%LOCALAPPDATA%\GameOCR\runtimes`：NVIDIA 库放在 `nvidia`，ORT GPU 和 llama.cpp Worker 放在 `backends`，子目录包含版本和归档哈希。每次 GPU 加载前重新验证。`GAMEOCR_DATA_DIR` 可重定向测试数据。只为 GPU 子进程设置库搜索路径，ONNX 另用绝对 `ORT_DYLIB_PATH` 指向配套 GPU 主库，不修改系统 PATH、注册表或安装显卡驱动。

Auto 只使用已有完整组件，缺失或损坏时展示原因并回退 CPU。显式 CUDA 会准备或修复组件；下载失败保留可见错误，之后仍可卸载并切回 CPU。Adaptive 暂不支持显式 CUDA，下载前拒绝该组合。旧完整离线包中已有的 GPU Worker 和 NVIDIA 库继续优先使用。

可在 EXE 旁的 `runtime-downloads` 目录放置匹配的原始 ZIP，文件名必须与固定 URL 的最后一段一致。程序使用相同的大小、SHA-256 和提取检查安装该文件，不修改原 ZIP。此方式可用于离线分发配套 GPU 资源；首次使用仍需已有 NVIDIA 驱动及所有所需组件。

## 构建与发布

先生成配套 GPU 资源，再构建主程序，两者必须配套发布：

```powershell
./scripts/build-workers.ps1 -Backend both -Profile release
node scripts/collect-licenses.mjs
# 正式发布时传入实际 Release 标签；CI 使用当前 Git 标签
./scripts/package-gpu-backends.ps1 -ReleaseTag vYOUR_VERSION
# 上一步在当前 PowerShell 设置变量；换终端后需重新设置
$env:GAMEOCR_GPU_MANIFEST=(Resolve-Path artifacts/gpu-backends.json).Path
npm run tauri -- build --no-bundle --config src-tauri/tauri.portable.conf.json
./scripts/package-portable.ps1 -Backend ondemand
./scripts/verify-package.ps1 -Backend ondemand -PortableOnly
```

不指定标签时，本地脚本使用 `gpu-<归档哈希前12位>`。`artifacts/gpu-backends.json`、配套 GPU ZIP 和 SHA256SUMS 一起保存；基础包打包前通过 EXE 的 `--gpu-catalog` 检查内嵌清单与资源清单一致，防止忘记重新构建主程序。不要用普通 Cargo 构建覆盖 Tauri 的正式 GUI。

CI 会将配套 GPU ZIP 上传到同一个 Draft Release；只有发布该 Release 后，匿名用户才能从程序中的固定 URL 下载。**2026-10-23 本地验证时，仓库没有公开 Release，本地配套 llama.cpp ZIP 尚未上传发布。** 因此本轮证明的是配套资源的离线安装及运行，不能声称 GameOCR 公网下载已通过。分发基础包前应发布匹配资源；否则首次 llama.cpp CUDA 加载会报告下载失败。Microsoft/NVIDIA 不依赖本项目资源发布。

NSIS/MSI 已采用同样的精简资源配置；本轮仅重建便携 ZIP，没有重新生成安装器。标准包仍保留现有 CPU 后端，CPU 可离线使用；进一步移除 CPU 库会失去这一能力。

## 验证与复现

```powershell
cargo test -p gameocr --lib --locked
cargo build -p gameocr --features test-fixture --bin capture-fixture
# 需要交互式 Windows 桌面、NVIDIA GPU/驱动、真实模型及数 GB 空间
# 可选：复用此前从官方完整下载并验证的 NVIDIA 组件
$env:GAMEOCR_CUDA_SEED='<已有缓存>/runtimes/nvidia'
node scripts/desktop-cuda-runtime.mjs
```

桌面脚本从实际精简包复制隔离目录，清除开发机 GPU 库路径，验证启动/Auto 不下载、Microsoft ORT 下载取消与续传、Small/Medium CUDA WGC 识别、配套 llama.cpp ZIP 离线安装和 GGUF WGC 识别。随后移除离线 ZIP，以不可达代理重启，验证两种 Runtime 使用缓存。读取 Worker 模块绝对路径，确认实际库及 llama.cpp Worker 来自隔离缓存。

设置 NVIDIA 缓存种子时，报告明确列出复用项，不算作本轮下载；此前六项 NVIDIA 完整官方下载见 [原始下载记录](test-results/cuda-runtime-download.json)。桌面结果保存为 `docs/test-results/gpu-backends-desktop.json`。应用采用单实例机制，所有桌面脚本必须串行执行。静态依赖、开发机 PATH 隔离和本机 GPU 验证不等同于干净 Windows 或其他 GPU 的验收。
