# 自适应 OCR 调度

实现日期：2026-10-19，分支 `feature/multi-ocr-runtime`。这是现有 Executor 上的增量调度层，不修改 PP-OCRv6 Adapter、原生 CUDA 参数或 GGUF 推理。Adaptive 默认关闭；本报告区分算法测试、真实模型执行和合成语料的性能评估。

## 配置与兼容

`executionMode` 为 `single` / `manual` / `adaptive`。缺字段的旧配置依据 `enableParallelInference` 迁移为 Single 或 Manual；显式模式优先。新配置默认 Single。`adaptive` 默认 `learning=true`、`exploration=true`、`maxCpuWorkers=2`、`maxGpuWorkers=0`。禁用学习时仍可使用历史，但不探索、不写入；清空是用户显式操作。

第一阶段只启用 `cpu-single` 和 `cpu-parallel-2`。线程总预算取用户设置与可用逻辑核数的较小值：例如 CPU×1 使用 4 线程、CPU×2 每实例 2 线程。资源不足两个线程或 CPU 上限为 1 时仅使用单实例。GPU 上限字段预留，非零尚不支持；Adaptive 的 Auto 明确选择 CPU，显式 CUDA 返回设置错误。Single / Manual 的设备选择及 GPU 回退保持原行为，GGUF 忽略自适应模式。

设置中可查看学习统计或清空档案；执行面板显示实际策略、CPU/GPU 数量、分析/决策/切换/推理耗时、相似样本、历史中位、预测评分、选择原因、探索与整图复验状态。没有显示未经可比基准证明的加速比。学习统计中的最近延迟是成功帧完整执行时间的 EMA；不混合候选稳态耗时。

## 传统视觉特征

`FrameAnalyzer` 在最长边 320 的灰度缩略图上工作，保持长宽比，从原 RGB 缓冲采样而不复制全图。没有额外神经网络、Python 或 OpenCV 运行库：

- 64 位 dHash（JSON 用十六进制字符串）、16 桶灰度直方图、与上一帧的变化比例。
- 3×3 Sobel 梯度近似、边缘密度、灰度方差、积分图上的局部均值与对比度。
- 双极性局部阈值、四邻域连通域、尺寸/比例过滤，以及横纵投影；兼容横竖候选，但不将其解释成真正的文字检测。
- 4×4 网格的边缘、文字候选、对比度与灰度摘要，活跃区域、象限熵和负载不均衡度。
- 复用 FrameSplitter 的 `quiet_seam` 候选位置，计算接缝梯度、文字密度及跨线区域风险。

64 维标准化向量采用加权距离；dHash 辅助近图匹配，不是唯一条件。当前未实现 pHash。变化检测仍沿用原 64×64 RGB 检查，切片也保留自身安全检查；本轮只复用了接缝算法，尚未统一三者的缩略图缓冲。特征在阻塞线程计算，不在 UI 线程执行。

Sobel 和局部阈值的算法依据可参考 [OpenCV Sobel 说明](https://docs.opencv.org/4.13.0/d2/d2c/tutorial_sobel_derivatives.html) 与 [阈值说明](https://docs.opencv.org/4.13.0/d7/d4d/tutorial_py_thresholding.html)。透明 UI、艺术字、纹理背景仍可能误导这些启发式，因此不据此声称准确率提升。

## 历史、匹配与在线决策

`AdaptiveProfileStore` 首先寻找近似画面（dHash Hamming≤6、面积比例 0.9–1.1、距离<0.055、直方图 L1<0.3），否则取距离≤0.075 的最多三个工作负载邻居。没有可信匹配就返回未知。记录保留近图样本，避免把每个 kNN 命中都无条件合成同一类。

每类每策略记录成功/失败/重试/整图回退/风险次数、文字块数量 EMA、延迟 EMA、偏差与 128 桶对数直方图。每次成功观测按 0.98 衰减旧分布；桶宽约 10%，UI 历史中位与 P95 是近似值。评分为 0.8×中位+0.2×P95，再加失败、重试和质量风险惩罚；整图复验时间已计入实测，不删除保护来制造收益。

两个策略均至少 5 次成功且有效样本≥4.5，CPU×2 评分至少低 10%、波动可控且无新增风险时，才提升并行偏好。单次更快不能改变可靠偏好。新帧持续三次显著偏离历史均值（>1.5 倍或<0.6 倍）时将旧直方图和有效样本权重降至 20%，重新校准。

探索采用确定性的 ε 节奏：每 20 个合适机会最多一次，全局和同类 30 秒冷却；只试用一帧。CPU 负载>85%、可用内存低于 max(512 MiB, 模型大小×6)、上一帧>1500 ms、连续失败或高风险画面会暂停探索。关闭学习也关闭探索。此机制在新的相似帧上采样，不把相邻不同图片称为严格 A/B。

正常策略切换有 10 秒冷却，并将已测切换成本按 20 帧摊销，与历史收益比较；高风险返回单实例可越过冷却。算法 v2 保留当前视觉风险下限：旧相似画面可能根本未被切片，其零回退率不能抹去当前集中布局的风险。摊销是假设而不是对未来停留时间的保证，快速交替画面仍可能使重载代价超过推理收益。

## 进程与质量保护

`ExecutionStrategyManager` 一次仅保留一套 Executor。完整单实例与两个半预算实例不能共享相同的 Session 线程配置，因此采用按需停止旧池、重建与预热新池。维持原配置跨帧复用，避免多个候选池同时自旋。线程自旋会占用 CPU，相关机制见 [ORT 线程配置](https://onnxruntime.ai/docs/performance/tune-performance/threading.html)；本轮未修改原 Worker 的 ORT 参数。

实际并行继续调用原 FrameSplitter/ResultMerger：恢复 bbox/polygon、保持原生置信度、去重、Worker 故障重试及边界整图复验。历史文字块数量异常突变、失败、重试和整图回退作为风险信号。没有可信参考时，高置信度不能证明未漏字。局部边界 ROI 复验尚未加入，无法证明完整性时保留整图保护。

取消、暂停、捕获目标变化及卸载后的失效帧不提交学习结果。档案清空有独立代数屏障，清空之前已经开始分析的帧不能重新写回。切换途中取消会完成替代池初始化并保留可用模型，当前帧仍被拒绝；退出会关闭本应用拥有的 Worker。

## 持久化与隔离

默认 `%LOCALAPPDATA%/GameOCR/adaptive-profiles.json`，测试尊重 `GAMEOCR_DATA_DIR`。结构包含 schema、featureVersion、algorithmVersion、nextId、profiles 和 summary。每个 profile 只有数值特征、指纹、聚合统计与时间，不含截图或 OCR 文字。

最多 256 类，文件读取上限 4 MiB；匹配忽略超过 30 天的记录，写入时清理超过 90 天及最旧记录。内存索引线性扫描有固定上限。单独写线程以 250 ms 合并更新并原子替换，清空/退出可刷新。损坏、不兼容版本或越界结构回到安全默认状态，并在统计中显示恢复状态。

SHA-256 指纹覆盖模型 ID/版本、各文件权重标识、Runtime/Adapter/CPU Provider、全部推理参数、线程预算、CPU 标识/逻辑核数、OS、应用/算法/特征版本，以及实际 CPU Worker 和 ORT DLL 字节。Batch、检测分辨率、模型或 Worker 更换不能直接复用旧统计。当前 GPU 为 disabled，不暗示已有 GPU 性能档案。

## 复现与测量口径

准备已有官方 Small/Medium 模型及 CPU Worker 后运行：

```powershell
cargo test -p gameocr-core --no-default-features --locked
cargo test -p gameocr-onnx --locked
cargo test -p gameocr --lib --locked
npm run check
npm test
cargo build -p gameocr --features test-fixture --bins --locked
node scripts/desktop-adaptive.mjs
cargo build -p gameocr --features test-fixture --bin executor-bench --release --locked
node scripts/test-adaptive.mjs
node scripts/benchmark-adaptive.mjs
```

基准运行时不要并发编译、其他 OCR 测试或修改源码。桌面开发测试运行时也避免修改 Vite 监听的文件；正式包可设 `GAMEOCR_APP` 测试内嵌前端。报告在 `docs/test-results`；独立临时档案在 `.cache/adaptive-benchmark`，不使用用户真实历史。

学习集采用既有 8 张 parallel 合成图，每个策略每张图 2 次预热+7 次实测；相同原图、参数、总线程预算串行比较。只有实际测得的结果通过开发专用 executor-bench `observe` 录入，正式 UI 不暴露该接口。输出分歧只标记不确定性，不默认并行或单实例正确。

评估集为 7 张独立 PNG：保留布局但更换文本/位置的 HUD、对白、菜单，反色新风格、未知中心布局、跨界长句、空白。检查学习与评估图 SHA-256 不重合。每种模式每图至少 5 次，加 24 帧交替布局；固定 CPU×1、固定 CPU×2、冷启动 Adaptive、只读历史、学习但无探索、学习且探索分别使用隔离档案。无评估预热，计入首次形状开销和全部切换。

wallMs 包括特征、决策、模型切换、传输、OCR、合并与清理，不含语料 PNG 解码、WGC 捕获、结果 JPEG 或浏览器绘制；真实桌面面板另包含结果 JPEG。冷模型加载单列。资源采样包含主进程及当前 Worker；已退出 Worker 的累计 CPU 时间不可得，不编造总体 CPU 利用率。GPU 采样为整卡，包含其他应用。

另设独立有限 CPU 争用→恢复实验，不混入六模式延迟比较。只终止测试自己创建的线程，记录实际负载和是否触发重新校准；未触发不能称为已验证真实漂移恢复。

## 适用范围

合成图的 CER、行召回及 bbox 偏差不能推广到真实游戏；标签是合成字体框近似，不是人工精修检测框。未验证 RTX5060Ti、干净 Windows、真实 Android/模拟器本轮重连、HDR、多 DPI 或受保护游戏。没有新增 GPU 探索、额外神经模型、ROI 缓存或后台重复 A/B。默认仍为 Single，用户可随时卸载模型后恢复手动选择。
