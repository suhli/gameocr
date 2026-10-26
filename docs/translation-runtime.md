# Translation Runtime

首个 Adapter 是 `hy-mt2`，Runtime 标识是 `llamacpp-translation`。桌面应用只链接无推理特性的 Rust 类型和调度模块；CPU/CUDA 原生代码分别位于 `workers/translation/cpu/translation-worker.exe` 和独立 GPU 组件中。没有 HTTP 服务、Python、Ollama 或 LM Studio 运行依赖。

## 已验证模型

- 来源：[腾讯官方 Hy-MT2-1.8B-GGUF](https://huggingface.co/tencent/Hy-MT2-1.8B-GGUF)。
- Revision：`a0c709d9fac510f2c807aa3af52872340dc37a4a`。
- 文件：`Hy-MT2-1.8B-Q4_K_M.gguf`，1,133,080,448 字节。
- SHA-256：`dc5f44fcf1fa496ee7ad725982c0c8c553a4de00259b53af84c4b89fb0c06699`。
- 许可证：官方该 revision 的 `LICENSE.txt` 声明 Apache-2.0；完整文本在 `licenses/Hy-MT2-LICENSE.txt`。
- 实际 GGUF：版本 3，`hunyuan-dense`；GPT2 tokenizer，`hunyuan-dense` pre-tokenizer；354 个张量，129 个 F32、192 个 Q4_K、33 个 Q6_K。

固定 `llama-cpp-2 = 0.1.159` 已能加载并生成 CPU/CUDA 翻译，无需升级 OCR 的 llama.cpp/MTMD。模型自带 Hunyuan chat template，经 `apply_chat_template` 处理；仅传一个 user message，无默认 system。初始化时检查架构、模板及实际 decode，生成时使用模型 EOG 检查。输出达到预算会标记 truncated，不冒充完整译文。

官方提及的 [STQ kernel PR](https://github.com/ggml-org/llama.cpp/pull/22836) 针对特殊 ternary/STQ1_0 量化；本次 Q4_K_M 文件不含该类型。不能据此推断其他量化版本也兼容。本地导入当前只接受与已核验官方 Q4_K_M 相同的文件；其他 GGUF 需新增清单和兼容性证据。

## 生命周期与 Prompt

Worker 内一个线程拥有 backend、model 和长期复用的 context，避免自引用结构和泄漏。每个请求前后清空 KV cache，受控上下文只通过 Prompt 传递。采样器按请求创建，默认温度 0.7、top_p 0.6、top_k 20、repeat penalty 1.05、seed 42。游戏默认生成上限 512、上下文 4096；官方示例的 4096 输出上限不是这里的默认值。可配置预算，但总 Prompt+输出必须容纳在上下文中。

Prompt 支持目标语言、风格、术语、背景、已确认对白、换行与数字保留。没有默认游戏术语。语言菜单使用官方明确列表；实际本地语料验证范围是日英中，其他语言并未逐一验收。

## 获取与设备

翻译文件位于数据目录 `translation/models`，独立清单无 detector、recognizer 或 mmproj。下载复用现有 Range、长度和 SHA-256 校验，支持 HF-Mirror、取消后续传。加载前再次校验完整文件。已加载模型不能删除。

显式 CUDA 会先检查驱动可用显存，再准备 GPU Worker 和 NVIDIA 组件，失败返回清楚错误。Auto 仅尝试已准备好的 GPU 后端，不会自动启动大型下载，失败回退 CPU。CPU Only 覆盖翻译设备选择；OCR 设备仍由 OCR 设置控制。

显存估算由权重、当前 Token 预算的 KV 和额外缓冲组成。默认预留 512 MiB，Balanced 至少额外预留 2048 MiB；其他策略允许配置 128–8192 MiB 预留。这是准入估算，不是真实显存占用或游戏性能保证；实际加载仍可能因碎片或并发分配失败。Worker 失败退出后释放其进程资源。

CPU Worker 加入基础发行包，权重和 CUDA Worker 不加入。`package-gpu-backends.ps1` 生成 OCR 与翻译两个独立 ZIP 和嵌入式校验清单。需先公开发布对应 Release 资源，用户才能联网获取自有 Worker；本地验证可放置匹配 ZIP 到 `runtime-downloads`。Microsoft/NVIDIA 组件仍走原官方来源与校验流程。

## 复现

```powershell
./scripts/build-workers.ps1 -Runtime translation -Backend cpu
target/workers-cpu/release/translation-test.exe --model .cache/translation/Hy-MT2-1.8B-Q4_K_M.gguf --input tests/translation/corpus.json --output artifacts/translation-cpu.json
./scripts/build-workers.ps1 -Runtime translation -Backend cuda
# CUDA CLI 的 PATH 需包含已准备好的 CUDA DLL 目录；桌面程序自动为子进程配置。
target/workers-cuda/release/translation-test.exe --device cuda --model .cache/translation/Hy-MT2-1.8B-Q4_K_M.gguf --input tests/translation/corpus.json --output artifacts/translation-cuda.json
```

本机 CUDA 12.1 开发验证额外使用 `-LocalCuda121Compatibility -CudaArchitectures '75;80;86;89'`。发布 CI 使用 CUDA 12.8.1；不能把 GTX1650 的通过记录当作 RTX5060 或干净虚拟机验证。
