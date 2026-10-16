# 多 Runtime OCR 架构

GameOCR 保留 Tauri 2 + Svelte 5 + Rust，以及既有 WGC / ADB 捕获与单槽调度。Runtime 负责执行模型，Adapter 负责模型语义；文件扩展名不能决定网络是否兼容。没有动态插件 ABI，也没有推理 HTTP 服务。

## 数据流与关键接口

```text
WGC / ADB → Frame → 最新帧槽 / ChangeDetector → OCR Service
  → RuntimeRegistry 验证 Manifest、能力、配置和文件角色
  → WorkerManager 按 runtime + device 选独立进程
  → NDJSON LoadEngine v2 → EngineFactory → Box<dyn OcrEngine>
       lightonocr → llama.cpp / MTMD
       ppocrv6    → ONNX Runtime detector + recognizer Sessions
  → OcrResult → 识别帧、bbox 联动、历史、复制与导出
```

`crates/gameocr-core/src/engine.rs` 定义以下契约：

- `OcrEngine`：`recognize`、`capabilities`、`identity`、`device`、消费式 `shutdown`。初始化通过工厂构造函数完成，释放由 Rust 所有权/Drop 完成。
- `OcrEngineFactory::create(EngineConfig)`：验证后调用所注册 Adapter 的构造函数。`EngineFactory::with_registry` 可接受扩展注册表，Worker 只注册自己真正实现的构造函数。
- `RuntimeRegistry`：Runtime ID → Worker 文件名；Adapter ID → 所属 Runtime、必需文件角色及能力。拒绝未知 ID、缺失文件角色、伪报能力或不匹配的配置。
- `EngineConfig`：Manifest、模型目录、设备、线程数及 `RuntimeConfig`。后者是带标签的 `Llamacpp(InferenceConfig)` / `Onnx(OnnxConfig)`，ONNX 不接收 Token、Prompt、温度参数。
- `OcrCapabilities`：plain、grounding、nativeCoordinates、confidence、generation。PP-OCR 的 grounding 表示可返回坐标；它始终来自检测网络，不生成 grounding Prompt。

`LightOnOcrAdapter` 包装原来的 MTMD 实现，保持 Qwen3.5 模板、图片 marker、采样、每帧独立上下文及 Grounding 解析行为。模型架构检查仍处于此推理实现内。`PaddleOcrAdapter` 拥有两个可复用 Session 和完整字典；运行过程见 [模型兼容性](model-compatibility.md)。

新增同 Runtime 的模型时，实现 Adapter、注册能力/文件角色/构造函数，添加经过验证的 Manifest 及相应输入输出测试。复用同一 Runtime 配置即可保留 Service 调度流程；如果新 Adapter 需要不同参数，应增加有类型的配置分支与对应设置面板。新增 Runtime 还需独立 Worker、专用配置、打包规则；通用 Service 不增加按模型名称判断的分支。

## Worker 协议与生命周期

```text
workers/
  llamacpp/cpu/ocr-worker.exe
  llamacpp/cuda/ocr-worker.exe
  onnx/cpu/onnx-worker.exe + onnxruntime.dll
  onnx/cuda/onnx-worker.exe + ORT CUDA provider DLLs
  cuda-runtime/  CUDA / cuDNN 运行库（仅存一份）
```

CPU 与 CUDA Worker 分别编译。UI 不链接 ORT、llama.cpp 或 CUDA。GPU 子进程 PATH 优先包含自己的目录和共享 cuda-runtime；CPU 子进程不添加 GPU 库目录。ORT CPU DLL 来自独立的官方 CPU 发行档，不是静默回退的 GPU DLL。

新消息使用 `{"type":"loadEngine","protocol_version":2,"config":...}`，后续 `recognize`、`unload`、`shutdown` 复用既有 NDJSON 契约。旧 GGUF `load` 消息仍由 llama.cpp Worker 支持；旧客户端不能把 ONNX 配置发给它。未知协议版本、错误配置和未加载请求返回 Error。stdout 只输出协议，诊断进入 stderr。

Auto 顺序是 CUDA → CPU。CUDA 必须成功创建两个 Session、各执行一次 warmup，并从 ORT profile 确认两个网络都实际执行 CUDA kernel，才发 Ready / CUDA。失败时父进程销毁它，再创建独立 CPU Worker，并展示失败原因。显式 CUDA 失败保持错误，不悄悄回退。ORT 可以把不支持的个别算子放到 CPU，显示 CUDA 表示网络实际有 GPU 执行，不承诺每个算子都在 GPU。

每次只执行一个请求。切换模型先关闭旧 Worker；卸载发 shutdown，等待最多 2 秒，必要时 kill 并 wait。单请求 180 秒超时后终止进程；退出标记每 50 ms 检查。异常进程、错误响应、EOF 不会把原生运行库异常传播为主 UI 进程崩溃。stdout 单响应上限 16 MiB；stderr 读取即分成最多 4096 字节的片段，只保留 128 片，避免没有换行的诊断导致无界分配。

## 可选并行执行器

`Service` 持有 `Executor`；内部 `Strategy::Sequential` 复用原 Worker/recognize 路径，`Strategy::Parallel` 持有 `WorkerPool`。只对 ONNX/ppocrv6 开启并行。具体检测、批量识别和 CUDA 配置仍在 Adapter，主进程不链接推理库。

`ParallelConfig` 作为 settings.json 顶层字段持久化。默认关闭，CPU 与 CUDA 模式分别只启用对应类型；Auto 可显式混合或在 CUDA 不可用时回退。CPU 总线程预算按实例分摊，GPU Worker 的 CPU 侧线程设为 1；GPU 上限 2、CPU 上限 min(4, 逻辑核数, 线程预算)。启动 GPU 前检查 GPU0 可用显存，Small/Medium 最少分别需要 512/1024 MiB 余量；这是保守准入检查，不是精确峰值预测。显式 CUDA 任一实例加载失败便失败；Auto 可保留成功的实例并报告实际数量。卸载等待所有进程退出，之后才允许换配置。

`gameocr-core/src/parallel.rs` 的 `FrameSplitter` 在至少 1280×720、至少两个 Worker、多个区域有足够对比度时生成 2×2 网格。接缝在中间 45–55% 区间寻找较低梯度位置，两侧各扩展 64 px。每块检测尺寸按原图比例向上取整到 32 的倍数，避免四次完整检测预算。小图、空白和局部稀疏图保留整图。此对比度检查不是额外文字检测，也不生成 OCR 文本。

`WorkerPool` 的有界动态队列最多 4 个任务，每个 Worker 最多一个在途请求。每个实例先获得真实任务以估计毫秒/百万像素，之后用 EMA 更新速度；快实例完成后领取下一任务，已测出的慢实例在预计拖慢队列时等待。不同画面的文字量仍会影响预测，调度不保证单调加速。每帧源 RGB 通过 Arc 共享；每批 UUID 临时目录中按源行直接写独立 PPM 文件，不复制整块 RGB，不压缩 PNG；正常、失败和取消均由 RAII 清理。没有引入共享内存、额外预处理线程或不可靠 ROI 缓存。

新增 `recognizeTile` / `tileResult` 消息携带 batchId、taskId、epoch、generation，外层保留 frame_id / timestamp。接收时校验全部身份与局部尺寸，模型身份也必须一致。旧 recognize 消息保持不变；并行需要新版 ONNX Worker。失效实例关闭，失败切片最多在另一个实例重试一次；最终成功任务数必须等于计划数，不能发布半帧。

`ResultMerger` 恢复 bbox/polygon 到原图坐标，按位置交叠和文本相似性去重，优先完整长文本及其原始置信度，按文本行顺序输出。检测框触及内部边缘，或分片只有不足两个块时，受控重跑整图以保护跨界句子。整图复验也会计入端到端耗时与任务数。只生成一条最终 OcrResult/history，继续复用现有预览、Grounding 和导出。

捕获仍采用一个最新待处理帧槽。新截图覆盖尚未受理的旧截图，不为每张新截图建立切片队列；已受理批次在同一 epoch/generation 内完成，避免高 FPS 下每批都被取消而永远无结果。暂停、停止、窗口切换、卸载会使批次失效：立即停止领取剩余任务，排空正在运行的原生请求，最终发布前再次检查 epoch/generation。退出标记会使请求终止子进程。此策略保留原先 latest-frame-wins 的受理语义。

`Metrics.execution` 与 `Performance.execution` 增加传输、切片、最长排队、Worker 墙钟、合并、完整帧耗时、任务计数和每实例状态。并行原生阶段耗时是工作量之和，不能相加当成延迟。benchmark 的 wallMs 包括临时目录清理；UI 的 execution.endToEndMs 还包含生成对应结果 JPEG，均不含截图采集前的时间和浏览器绘制。原单 Worker totalMs 保持引擎时间；并行 totalMs 使用 Executor 墙钟。speedup 仅在独立 benchmark 找到相同图像/参数真实基线后计算，实时 UI 不编造基线。

## 捕获、取消和内存

WGC 控制线程持有 free-threaded capture session；回调检查 generation、暂停和 FPS 限流后将 RGBA 转 RGB。ADB 使用可取消的 screencap，两者共用 Frame。HWND、PID、创建时间、窗口类和销毁代数用于目标身份检查，不能把重建窗口当作旧窗口。

只保留最新待处理帧与当前预览；新帧覆盖待处理帧并记录丢弃计数，不排队累积。JPEG 预览最长边 1280、质量 78、最多每秒 4 次。64×64 RGB 差值用于变化检测，手动单次识别绕过该检测。

停止 OCR / 切换目标立即更新 epoch，原生调用可继续完成，但旧结果不发布；发布和 generation 更新在同一短临界区。历史长度受设置上限约束，保留各自压缩预览，仅最新识别帧保留完整 RGB。旧历史可导出 JSON，原始 PNG 仅针对最新识别帧或当前画面。

ONNX 两个 Session 跨帧复用，识别按宽高比分组批处理，不为每个框建立 Session。限制检测最长边 2048、候选 3000、Batch 1–32、识别宽度 3200、输入解码分配 256 MiB。极大图/长文本仍可能占用较多显存；OOM 由独立 Worker 的错误/退出处理，不在 UI 线程推理。

Tauri 命令将模型扫描/哈希、导入复制、图像编码移入阻塞线程。模型文件操作串行；加载、删除、配置变更使用生命周期锁；模型已加载时禁止修改推理参数。

## Manifest 与模型目录

默认 `%LOCALAPPDATA%\GameOCR\models`，用户可在设置里选择其他模型目录。源文件选择后复制导入，不把用户源文件当作可删除的缓存。

```text
models/
  Q8_0.json                         # schemaVersion: 2
  LightOnOCR-3-0.8B-Q8_0.gguf        # role: main
  LightOnOCR-3-0.8B-mmproj-f16.gguf   # role: mmproj，可共享
  ppocrv6-small.json
  ppocrv6-small-det.onnx            # role: detector
  ppocrv6-small-rec.onnx            # role: recognizer
  ppocrv6-medium.json
  ppocrv6-medium-det.onnx
  ppocrv6-medium-rec.onnx
  ppocrv6_dict.txt                  # role: dictionary，两规格共享
  import-<uuid>.json                # 导入文件使用唯一副本名
```

Manifest v2 声明 ID、名称、runtime、adapter、版本、精度、能力、files、来源、parameters、imported/installed。每个 file 声明 role、相对单文件名、大小、SHA-256、可选下载 URL。禁止路径穿越与重复角色。未来 classifier、config、tokenizer 可作为额外角色，不需要改变模型管理器字段。

旧 GGUF JSON 的 modelFile/mmprojFile 等字段转成 main/mmproj 角色，保留模型 ID、路径、哈希、导入状态，通过原子 JSON 写入迁移；权重原地复用。旧配置缺少 onnx 字段时使用默认值。历史 OcrBlock/OcrResult 新字段使用 serde 默认值，旧 JSON 仍可读。

下载使用 `<原文件名>.part`，校验 206 offset/总大小；200 会重新写入，避免错误追加。断网保留部分文件重试，完成后大小和 SHA-256 均通过才发布。模型列表也重新校验哈希，防止等长损坏显示已安装；大 GGUF 较慢，扫描在后台执行。导入 ONNX 使用 CPU warmup 验证检测/识别网络和完整字典维度，发布前失败回滚本次副本。删除检查其他 Manifest 的所有引用，包括暂时损坏/未完整模型的共享文件；不会删除用户原始导入来源。

## 统一结果与修改位置

保留 bbox、rawOutput、truncated 和原有 Metrics。增加可选 confidence、polygon、runtime、modelId、engineId、detectionMs、recognitionMs。坐标始终是原图像素；bbox 是 polygon 的轴对齐外接矩形。GGUF 没有原生置信度，返回 null；ONNX 返回 CTC 置信度与四点框，rawOutput 是空串、truncated 为 false。前端继续以 bbox 联动，四点信息保留于 JSON。

- `gameocr-core/{engine,manifest,worker,types}.rs`：契约、迁移、工厂、协议；`inference.rs`：原 GGUF 行为和 Adapter。
- `gameocr-onnx/{lib,pipeline}.rs`：ORT 加载、EP 检查、图像处理、DB、透视、批量识别、CTC；`bin/`：Worker / CLI。
- `src-tauri/src/{model,ocr,service,settings,commands}.rs`：模型存储、进程策略、调度接线与 IPC；`catalog/ppocrv6.json`：固定官方下载元数据。
- `src/{App,components/ModelManager,components/SettingsPanel,components/ExecutionStats}.svelte` 与 `src/types/index.ts`：能力驱动 UI、导入、实际设备、置信度与执行统计。
- `scripts/`、CI：原生依赖收集、固定版本下载、真实模型/桌面测试、发行包检查。

Metrics 的 preprocessingMs 包括图像解码、缩放、裁剪、识别张量准备；detectionMs / recognitionMs 是实际网络运行；parsingMs 包括 DB 与 CTC；inferenceMs 为两个网络耗时之和。单 Worker 的 totalMs 含引擎调用内全部耗时，不包括 WGC、发送前 PNG 编码和 UI JPEG；并行与完整端到端口径见上文。实时 UI 显存遥测仍显示不可用；独立 benchmark 可采集 nvidia-smi 整卡采样，明确包含其他应用占用。
