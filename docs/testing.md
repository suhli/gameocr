# 测试记录

日期：2026-10-12 / 13。以下区分真实执行、待完成和没有环境的项目；编译成功不等于 GPU / 游戏兼容性通过。

## 环境

Windows 11 x64 开发机，GTX 1650 4 GB，NVIDIA 591.86 驱动，CUDA12.1，VS2022/MSVC14.41，Rust1.97.1，Node24.21，系统 WebView2。CUDA12.1 编译使用了构建脚本中显式的本机兼容开关；CI固定CUDA12.8.1，没有冒称CI GPU实测。

## 已实际通过

- `npm run check`：Svelte / TypeScript 0 errors、0 warnings；`npm test`：2项坐标/缩放测试；Vite生产构建成功。
- `cargo test -p gameocr-core --no-default-features --locked`：6项测试，覆盖Grounding、中英日混排、非法坐标、空/截断文本、帧变化与有界最新帧槽。
- `cargo test -p gameocr --lib --locked`：9项测试，覆盖窗口身份、配置原子持久化、导入回滚/校验/删除，以及真实loopback HTTP连接中断续传、忽略Range重下、校验失败不发布正式模型。
- `cargo clippy --workspace --all-targets --no-default-features --locked`：无错误，4项代码风格建议（嵌套if与默认值初始化），未将它描述为零警告。
- Q8_0 + 对应F16 mmproj真实CPU和CUDA图像推理；Grounding返回原始结构化文本与像素坐标，CLI重复识别复用模型、重建上下文。没有模拟OCR结果。
- Release CPU普通OCR：800×320测试图连续两次识别，第二次4.57秒；Release CPU Grounding第二次7.26秒。CUDA早期CLI Grounding第二次1.68秒。不同运行时背景负载与输出模式不同，不作为严格横向基准。
- 真实Tauri WebView挂载、模型导入与加载、Win32目标枚举与选择、WGC截图、CUDA识别、Grounding与对应预览帧、JSON及原始PNG导出。
- 中英日混排测试窗口可读，实际生成了6个区域。模型对标题栏等内容也会产生区域；准确率不是人工修正后的结果。
- 原始PNG查看、点击边界框联动文字块；静态画面去重；暂停/继续；WGC窗口缩放；推理中停止后旧结果不回写；最小化后停止并报错、恢复后重新捕获。
- CPU / CUDA Release工作程序与Tauri Release主程序成功编译。NSIS和MSI都已生成过真实安装文件。
- Release程序从独立文件夹运行，子进程PATH仅保留Windows系统目录并移除CUDA_PATH，仍能加载随包DLL并在CUDA上推理。这验证了本机对开发工具路径的独立性，**不能代替干净系统验证**。

## 长时间运行

第一轮开发版长测持续约700秒后，WebView连接关闭，按未完成记录；这次的主进程私有内存采样约66–70MiB，历史保持5条，但不据此声称30分钟通过。没有在对应时间段查到GameOCR崩溃或系统休眠事件，具体关闭原因未确定。

正式候选版正在执行新的1800秒测试，关闭变化检测以持续施加推理负载，历史上限5条，每10秒记录性能与历史数量，每分钟采样主进程、worker和WebView进程内存。完成结果在交付前更新。

证据位置（本地构建产物，未提交大型二进制/截图）：

- `artifacts/validation/cpu-grounding.*`、`cuda-grounding.*`：第一轮真实CLI输出。
- `artifacts/validation/cpu-plain.*`、`cpu-release-grounding.*`：Release CPU复验。
- `artifacts/validation/desktop-initial-interrupted.log`：第一轮中断日志。
- `artifacts/validation/desktop-release/`：Release端到端报告、截图、导出JSON和soak-progress.json。
- `.cache/final-*.log`：最终检查与打包日志。

## 待完成的最终检查

- 最终Portable内容、PE导入依赖和SHA-256清单。
- 1800秒连续运行结束后的卸载、窗口销毁与正常退出。
- 禁用GPU可见性后的自动CPU回退、错误模型加载恢复。
- 可用MuMu主窗口的真实WGC与CPU OCR。

## 未测试 / 当前限制

- 干净Windows11虚拟机启动、安装版系统安装/卸载、无NVIDIA驱动的物理机器。没有可用的干净VM；本机PATH隔离测试和CUDA_VISIBLE_DEVICES测试不可冒充这些项目。
- DirectX / OpenGL / Vulkan游戏实际渲染窗口、反作弊或受保护画面；不绕过系统保护。
- Android游戏渲染窗口及ADB PNG捕获/断开重连。找到MuMu自带ADB并实际执行devices，但当时设备列表为空。
- 其他GPU型号、多显示器不同DPI、HDR；坐标映射有自动化测试，本机窗口缩放已测试。
- Q4_K_M、Q5_K_M、F16模型量化的真实推理与质量对比：目录大小/哈希已核对，实际推理验证采用Q8_0。
- GitHub Actions workflow尚未在远端执行；已在本机执行其主要构建与打包流程。
- 安装文件未进行代码签名。

停止OCR立即取消结果发布，已经进入原生推理的一次调用会在后台完成，超时上限180秒；应用退出会主动终止worker。显存占用遥测显示不可用。UI的OCR耗时来自引擎内测量，不含WGC截图、发送前PNG编码和结果JPEG显示的额外时间。
