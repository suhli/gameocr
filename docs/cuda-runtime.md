# CUDA 运行库按需下载

标准便携包与 NSIS/MSI 构建配置不再携带 NVIDIA CUDA/cuDNN 运行库。首次启动与 CPU/Auto 加载不会下载；明确选择 NVIDIA CUDA 并加载模型时，才准备相应 Runtime 的组件。已有完整离线包的 `workers/cuda-runtime` 优先使用。CPU 专用包仍需更换为标准包才能使用 CUDA。

## 固定来源与范围

下载清单位于 [cuda-runtime.json](../src-tauri/catalog/cuda-runtime.json)，版本、归档大小和 SHA-256 来自 NVIDIA 官方的 [CUDA 12.8.1 清单](https://developer.download.nvidia.com/compute/cuda/redist/redistrib_12.8.1.json) 和 [cuDNN 9.8.0 清单](https://developer.download.nvidia.com/compute/cudnn/redist/redistrib_9.8.0.json)。固定使用 Windows x86_64；cuDNN 选择 CUDA 12 变体。ORT 的版本要求见 [CUDA Execution Provider 文档](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html#requirements)。

- GGUF：CUDA Runtime 12.8.90、cuBLAS 12.8.4.1，共 566,698,679 字节，约 541 MiB。
- ONNX：上述两项，加 cuFFT 11.3.3.83、NVRTC 12.8.93、nvJitLink 12.8.93、cuDNN 9.8.0.87，共 1,995,551,251 字节，约 1.86 GiB。

这些是完整官方 ZIP 的下载量，程序仅提取需要的 DLL 与许可证，不安装归档里的头文件、静态库或开发工具。两个 Runtime 共用已有组件。官方 `.com` 地址在本机重定向到 `developer.download.nvidia.cn`，仅允许这两个 NVIDIA 域名的 HTTPS redistributable 路径；不会从第三方 DLL 站点取文件。

GPU Worker、llama.cpp 的 GPU 实现及 ORT CUDA provider 仍随标准包提供，不能由 NVIDIA 运行库代替。因此标准包仍明显大于 CPU 专用包。运行时下载不包含显卡驱动；需预先安装兼容驱动。它不需要管理员权限，不修改注册表或系统 PATH。

## 下载与缓存

界面显示当前组件、下载字节及校验/解压状态，可以取消。中断后重新加载模型会尝试 Range 续传；服务端不支持 Range 时安全重下。最多自动重试三次。完整归档先校验大小与 SHA-256，随后在临时目录提取并验证所需文件，完成后发布该组件，最后删除 ZIP。失败或取消不会将未完成组件标为可用；已有有效组件保留，`.part` 留待续传。

缓存为 `%LOCALAPPDATA%\GameOCR\runtimes\nvidia\<组件>-<版本>-<归档哈希前12位>`。每个目录保存 DLL、许可证及 `installed.json` 文件哈希记录。再次使用时重新验证，全部有效即可离线加载。`GAMEOCR_DATA_DIR` 会同时重定向运行库缓存，便于隔离测试。只为 GPU Worker 添加这些目录；主进程和 CPU Worker 不添加 NVIDIA 库路径。

取消后可卸载错误状态的模型，再切换 CPU 或重新加载 CUDA。Auto 只尝试已有完整组件，缺失或损坏时给出原因并回退 CPU。显式 CUDA 会准备或修复所需组件；下载失败保持可见错误。ONNX Adaptive 目前不支持显式 CUDA，加载时会在下载前拒绝该组合。

## 构建与复现

```powershell
# 默认：保留两类 GPU Worker，NVIDIA 运行库在首次明确使用时下载
./scripts/package-portable.ps1 -Backend ondemand
./scripts/verify-package.ps1 -Backend ondemand -PortableOnly
# 可选的完整离线包
./scripts/package-portable.ps1 -Backend both
./scripts/verify-package.ps1 -Backend both -PortableOnly
```

标准包原生依赖检查仅允许清单中的 NVIDIA DLL 延后提供，其他依赖仍须在包内或系统目录解析。该静态检查不能证明 CUDA 实际运行。

```powershell
cargo test -p gameocr --lib --locked
cargo build -p gameocr --features test-fixture --bin capture-fixture --bin cuda-runtime-install
# 需要可交互 Windows 桌面、NVIDIA GPU/驱动、真实模型、网络和数 GB 空间
node scripts/desktop-cuda-runtime.mjs
# 已下载缓存的 GGUF CUDA 验证（另需 models/ 中的 Q8_0 和 mmproj）
$env:GAMEOCR_CUDA_CACHE='<已下载数据目录>/runtimes/nvidia'
node scripts/test-cuda-runtime-gguf.mjs
```

桌面脚本使用新的隔离数据目录、仅包含 Windows 系统目录的 PATH，以及没有 `workers/cuda-runtime` 的实际 Release 包。它真实下载、取消、重试，检查子进程加载 DLL 的绝对路径，再进行 WGC OCR，最后用不可达代理验证重启后的缓存复用。测试窗口与进程由脚本自身创建。模型使用 `.cache/test-models` 中已核对的 Small/Medium，输出记录在 [cuda-runtime-desktop.json](test-results/cuda-runtime-desktop.json)。应用采用单实例运行，桌面脚本须依次执行，并先关闭其他 GameOCR 实例。此脚本有真实下载成本，不纳入普通 CI。

GGUF 的独立验证仅将两个已下载组件加入 GPU Worker 的 PATH，以真实 Q8_0 模型识别英文图片，再读取该子进程已加载模块的路径，结果见 [cuda-runtime-gguf.json](test-results/cuda-runtime-gguf.json)。不依赖开发机 CUDA Toolkit 的 PATH。
