# Translation Pipeline 交付记录

在现有 `main` 分支增量开发，保留原 OCR Runtime、模型目录、旧导出及用户配置。下列结论来自本机实际结果；没有将 Mock 或跳过项计作模型通过。

1. **改动模块**：新增 `gameocr-translation` 核心/Worker/CLI 与桌面 `translation` 管理模块；在 Service、命令、Svelte 结果和设置界面接入独立管线；扩展下载清单、打包脚本、CI、第三方许可证与文档。
2. **引擎架构**：Factory 按 runtime + adapter 注册；Hy-MT2 纯文本 Worker 独立持有 model/context。共享下载校验和有界 IPC，不共享 OCR 子进程或模型上下文。未来 ONNX/其他 GGUF 翻译 Adapter 尚未实现。
3. **模型实际运行**：官方固定 revision 的 Hy-MT2-1.8B Q4_K_M 已完整校验，真实 CPU/CUDA 各运行 14 条日英中语料。实际 WGC→PP-OCR→Hy-MT2→中文 UI 已完成。
4. **llama.cpp 兼容**：现有 `llama-cpp-2 0.1.159` 能处理 hunyuan-dense、tokenizer、Hunyuan Chat Template 和 Q4_K_M；无需升级原 OCR/MTMD。STQ 的特殊内核要求不适用于这个已实测量化文件。
5. **设备**：OCR/翻译独立 CPU/CUDA/Auto。CLI CUDA 在 GTX1650 上通过；最终桌面测试显存不足，CPU+GPU/GPU+GPU 没有通过记录。显式 CUDA 报错，Auto 实际回退 CPU，默认策略保留游戏显存。
6. **性能**：CPU 原生初始化约 1.26 秒，CUDA 约 1.18 秒；短文本推理约 0.72–1.37 秒 / 0.20–0.38 秒。桌面单句 CPU 样例从截图到译文约 830 ms。模型初始化、后台组件准备、排队、TTFT、生成和缓存分开记录，详见 [性能](translation-performance.md)。
7. **质量**：日英中方向可实际生成译文，术语、多行菜单、数字有成功样例；拟声词残留假名、占位符被翻译、术语过度套用和不完整 OCR 的语义问题仍存在。人工评价字段保持 null，详见 [质量边界](translation-quality.md)。
8. **缓存与逐字更新**：有界 LRU、可选持久化、模型/参数/Profile/上下文隔离；重复 OCR 不增加模型调用，重启后仍命中。四次逐字更新只生成一次；三条菜单可合批一次生成，严格校验编号，不符合时逐组回退。暂停/目标切换拒绝失效结果，崩溃可恢复，退出清理进程。
9. **原 OCR 影响**：提交翻译不等待模型，OCR 原始文本/坐标/置信度和导出保持独立。原 ONNX/GGUF 生命周期切换通过，最终 CPU 包的 12 项原 OCR 桌面回归通过。尚未测量真实 3D 游戏争用下的帧率和长期负载，不能声称没有任何性能影响。
10. **Windows 发布**：标准便携约 19.85 MiB、CPU 便携约 19.18 MiB；NSIS 约 12.45 MiB、MSI 约 17.21 MiB。基础包不含权重、CUDA Worker 或 NVIDIA DLL。ZIP 与原生导入检查通过；干净系统安装、远端 CI、公开 GPU 资源下载未验证。
11. **未完成验收与限制**：当前代理令桌面官方模型下载失败，取消/Range 续传仅共享组件测试通过，真实端点未通过。GPU 配套 ZIP 尚未公开发布；本地导入仅支持已核验官方 Q4_K_M。测试是合成游戏文字，其他官方列出的语言没有逐一验收。无 Overlay、画面替换或 TTS。
12. **后续优先项**：在可用网络完成官方端点下载/续传；在显存充足的机器测 CPU+GPU、GPU+GPU 与真实游戏争用；发布匹配哈希的 GPU 组件；扩大真实游戏质量语料并处理占位符和术语过度套用；验证干净 Windows 安装及其他显卡。

主要证据：

- [桌面翻译：16 项通过及资源跳过记录](test-results/translation-desktop.json)
- [原 OCR：12 项桌面回归](test-results/translation-ocr-regression.json)
- [CPU 原始结果](test-results/translation-cpu.json)、[CUDA 原始结果](test-results/translation-cuda.json)、[低温度对照](test-results/translation-cpu-low-temperature.json)
- [合批输出](test-results/translation-batch.json)、[输出上限](test-results/translation-output-limit.json)
- [模型下载失败证据](test-results/translation-download.json)、[包体与校验](test-results/translation-packaging.json)
