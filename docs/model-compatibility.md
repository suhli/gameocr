# 模型与原生推理兼容性

核对日期：2026-10-12 / 13。

## 确认的接口

- 官方模型：https://huggingface.co/lightonai/LightOnOCR-3-0.8B
- 官方使用说明与 Grounding：https://github.com/lightonai/LightOnOCR
- 当前下载源：https://huggingface.co/Prathat/LightOnOCR-3-0.8B-GGUF
- Rust 绑定：https://github.com/utilityai/llama-cpp-rs

固定 crates.io 发布版本 `llama-cpp-2 = 0.1.159`，启用 `mtmd`；CUDA worker 额外启用 `cuda`。实际读取了该版本的 Rust API 和 vendored llama.cpp 源码，确认 Qwen3.5 (`qwen35`) 与 LightOnOCR 视觉投影支持。未手写 FFI，未运行 HTTP 推理服务器。

`MtmdContext` 持有已加载的视觉投影，`LlamaModel` 持有语言模型；两个对象由 worker 主线程独占。每张图通过 RGB bitmap、`tokenize`、`eval_chunks` 进入独立 `LlamaContext`，之后生成文本。视觉投影先于主模型和 backend 析构。不对上游对象实现不安全的 Send / Sync。

## Prompt

依据官方模型卡及 `chat_template.jinja`，使用单个 user turn、关闭 thinking 的 Qwen3.5 模板。user 内容为 MTMD 默认图片 marker，Grounding 模式在图片之后直接添加 `grounding`。普通模式不添加这个词。MTMD 自己补入架构需要的 vision delimiter，不能重复手工插入。

```text
<|im_start|>user
{mtmd_default_marker()}{grounding 或空串}<|im_end|>
<|im_start|>assistant
<think>

</think>

```

Grounding 生成 `![label](x1,y1,x2,y2)` 后跟文字。坐标范围 0–1000。解析器将其映射回输入原始截图的像素坐标，并裁剪越界值。坐标非法、缺失或输出截断时仍保留可读文本及全部 rawOutput。

## 实测模型对

- Q8_0：811,843,648 bytes，SHA-256 `b5c262bc905015e78002ea0e360bcbc7e6b3edd71d29a80857f677392f48eb15`。
- mmproj F16：204,987,136 bytes，SHA-256 `e77f2ebb738f5c7852e955f09440c407d9c54b56c6f6098900380706302e8742`。

CLI 在 CPU 和 GTX 1650 4 GB 上对 `tests/fixtures/english.png` 进行了两次连续识别，均返回真实文字和边界框。原始证据保存在本机 `artifacts/validation/cpu-grounding.*` 与 `cuda-grounding.*`。GPU 第二次约 1.68 秒；该数字属于这张 800×320 测试图与这台机器，不能作为任意游戏画面的承诺。CPU 早期测试第二次约 16.91 秒，使用了修复前放大到 1280 的路径，不能直接与 GPU 数字作同条件比较；现在只缩小超过上限的图像。

下载目录还包含实际存在且固定文件大小和 SHA-256 的 Q4_K_M、Q5_K_M、F16。目录提供支持不等于每个量化已完成质量和性能测试。

CUDA 工具链仅在构建机使用，运行包携带必要 CUDA12 DLL。主 UI 二进制不链接 CUDA，自动模式尝试 CUDA worker 失败后关闭它并创建 CPU worker；显式 CUDA 模式会保留清晰错误。当前实际使用 CUDA12.1 + MSVC14.41 的本机验证；CI 使用12.8.1，须按 CI 真正运行情况验收。
