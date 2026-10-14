# 模型与原生推理兼容性

GGUF 原始核对日期：2026-10-12 / 13；多 Runtime 与 ONNX 验证：2026-10-14。

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

## 原 GGUF 实测模型对

- Q8_0：811,843,648 bytes，SHA-256 `b5c262bc905015e78002ea0e360bcbc7e6b3edd71d29a80857f677392f48eb15`。
- mmproj F16：204,987,136 bytes，SHA-256 `e77f2ebb738f5c7852e955f09440c407d9c54b56c6f6098900380706302e8742`。

CLI 在 CPU 和 GTX 1650 4 GB 上对 `tests/fixtures/english.png` 进行了两次连续识别，均返回真实文字和边界框。原始证据保存在本机 `artifacts/validation/cpu-grounding.*` 与 `cuda-grounding.*`。GPU 第二次约 1.68 秒；该数字属于这张 800×320 测试图与这台机器，不能作为任意游戏画面的承诺。CPU 早期测试第二次约 16.91 秒，使用了修复前放大到 1280 的路径，不能直接与 GPU 数字作同条件比较；现在只缩小超过上限的图像。

下载目录还包含实际存在且固定文件大小和 SHA-256 的 Q4_K_M、Q5_K_M、F16。目录提供支持不等于每个量化已完成质量和性能测试。

CUDA 工具链仅在构建机使用，运行包携带必要 CUDA12 DLL。主 UI 二进制不链接 CUDA，自动模式尝试 CUDA worker 失败后关闭它并创建 CPU worker；显式 CUDA 模式会保留清晰错误。当前实际使用 CUDA12.1 + MSVC14.41 的本机验证；CI 使用12.8.1，须按 CI 真正运行情况验收。

## ONNX / PP-OCRv6（2026-10-14）

采用 `ort = 2.0.0-rc.12`，启用 std、load-dynamic、api-24；CUDA Worker 额外启用 cuda。该版本对应 ORT 1.24 API、最低 Rust 1.88，支持 CUDA12/13 组合；选择官方 ORT **1.24.3 CUDA12** Windows x64 归档，不跟随需要 CUDA13 的后续绑定变更。CPU 归档 SHA-256 `4fbfb85d0e9de9bb6fb8a9866a7cb477cbad404d889b236931bf3f5d547e5f48`，GPU 归档 `f2af33fea71f477027961b8027e7496ab01c6b962ae4f596bf8163192b03b326`。没有 Python 运行依赖。

来源：[ort 发布记录](https://github.com/pykeio/ort/releases)、[ORT 1.24.3 官方发布](https://github.com/microsoft/onnxruntime/releases/tag/v1.24.3)、[CUDA EP 版本要求](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html)。图像后处理使用固定 `imageproc 0.25.0` 与 lockfile 中的 `image 0.25.10`。

### 固定模型来源

使用 PaddlePaddle 官方 Hugging Face ONNX 导出；下载 URL 固定到 revision，实际下载后校验了下面的大小和 SHA-256。完整可机器读取元数据在 [ppocrv6.json](../src-tauri/catalog/ppocrv6.json)，不是未经核实的占位地址。

- [small detector](https://huggingface.co/PaddlePaddle/PP-OCRv6_small_det_onnx)：9,880,512 bytes；`d73e0058b7a8086bbd57f3d10b8bcd4ff95363f67e06e2762b5e814fe9c9410e`。
- [small recognizer](https://huggingface.co/PaddlePaddle/PP-OCRv6_small_rec_onnx)：21,159,378 bytes；`5435fd747c9e0efe15a96d0b378d5bd157e9492ed8fd80edf08f30d02fa24634`。
- [medium detector](https://huggingface.co/PaddlePaddle/PP-OCRv6_medium_det_onnx)：62,032,837 bytes；`eb13b44b25bb36f89528b68720af8a61d9cf381176107f465db1757b65d086e1`。
- [medium recognizer](https://huggingface.co/PaddlePaddle/PP-OCRv6_medium_rec_onnx)：76,554,979 bytes；`9c09abf0957f7968c7586464b7397b84ad2387a0497a351af40e9acc71b673ba`。
- [官方完整字典](https://github.com/PaddlePaddle/PaddleOCR/blob/b03f46425e8ff4442b268ce449e3eef758146cd4/ppocr/utils/dict/ppocrv6_dict.txt)：74,947 bytes；`b5f2bfe2bdd9448429e3e82b51c789775d9b42f2403d082b00662eb77e401c5d`。18,708 个字典项加 blank 和空格，识别输出 18,710 类。检查精确字典哈希和输出维度，避免字符顺序错位；不接受缩小测试字典。

HF-Mirror 设置只改写 Hugging Face 下载主机，revision 和文件哈希不变；GitHub 字典源不经过 HF-Mirror。模型和字典均按需下载，不嵌入应用发行包。网络受限时可从这些固定来源取得文件后通过 UI 导入。

### Pipeline 和行为参考

参考 [PaddleOCR PP-OCRv6](https://github.com/PaddlePaddle/PaddleOCR/blob/main/docs/version3.x/algorithm/PP-OCRv6/PP-OCRv6.md) 及模型仓库的 inference.yml、[PaddleX 检测处理器](https://github.com/PaddlePaddle/PaddleX/blob/develop/paddlex/inference/models/text_detection/processors.py)、[PaddleX 识别处理器](https://github.com/PaddlePaddle/PaddleX/blob/develop/paddlex/inference/models/text_recognition/processors.py)。

1. RGB 截图转 BGR planar float32；检测按最长边上限缩放、宽高对齐 32，使用 mean `[0.485,0.456,0.406]` / std `[0.229,0.224,0.225]`。
2. DB 概率图二值化，提取轮廓、最小旋转矩形和框内概率均值。默认 detection 0.2、box 0.45、unclip 1.4，最多 3000 候选。按矩形面积/周长扩展四点框，恢复原图像素坐标。
3. 透视校正按长边裁剪，纵向长框旋转 270°；按宽高比分组，识别高 48、宽至少 320、最多 3200，BGR 归一化到 [-1,1] 并补零，一批复用一个 Session。
4. CTC 去 blank/相邻重复，计算实际输出字符概率均值，过滤低于 0.5 的文字；按原始阅读顺序输出文字、bbox、四点 polygon、confidence。空图成功返回空 blocks。

采用 imageproc 的轮廓、最小矩形及透视双线性插值，不依赖 OpenCV DLL。与参考实现遵守相同输入通道、归一化、阈值、字典和 CTC 语义，但 Rust 图像库的整数框/边界插值与 OpenCV 不保证逐像素或逐分数相等。未实现弯曲文字轮廓输出、页面方向分类或全角度方向分类；四点旋转文本和小字有实际测试，不能据此承诺任意旋转/弯曲文本准确率。

### GPU 兼容性与 SM120

CUDA EP 与 CPU EP 分属两个 Worker 和 ORT DLL。只有 Session 成功且两个网络 warmup 的 profile 均出现 CUDAExecutionProvider 算子才标记 CUDA；仅检测显卡或成功加载 DLL 不算 GPU 通过。模型初始化失败由 UI Auto 重启独立 CPU 进程。

在 GTX1650 + cuDNN9.8 上，默认 cuDNN 算法搜索对部分动态识别宽度出现不支持错误。因此固定使用 ORT ConvAlgorithmSearch::Default 与 `conv_max_workspace=false`，选择 cuDNN fallback 算法，经过 small/medium 全流程实测。此选择优先稳定性，当前小图 GPU 耗时高于 CPU；未宣称 GPU 必然更快。

正式构建 CUDA12.8.1、cuDNN9.8.0.87，llama.cpp 默认 `75;80;86;89;120`。RTX5060Ti 的计算能力 12.0 对应 SM120，cuDNN9.8 矩阵将 Blackwell 列于 CUDA12.8 路径；因此不能沿用仅 CUDA12.1 的旧本机包来验证它。[NVIDIA GPU 列表](https://developer.nvidia.com/cuda/gpus)、[cuDNN9.8 支持矩阵](https://docs.nvidia.com/deeplearning/cudnn/backend/v9.8.0/reference/support-matrix.html)。

本机仅有 GTX1650 和 CUDA12.1。已实际运行 CPU/CUDA；**RTX5060Ti、CUDA12.8 CI 构建与远端 CI 执行尚未验证**。编译架构列表和官方兼容矩阵是支持准备，不能代替硬件测试。所有结果和限制见 [测试记录](testing.md)。
