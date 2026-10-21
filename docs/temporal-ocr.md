# 时序 OCR

时序识别是 PP-OCRv6 的可选实验策略，默认关闭。它扩展现有 `FrameCache`，没有第二套独立缓存。缓存保存原 RGB、检测区域、四边形、文字/置信度，以及每行的边缘数量、周围亮度、连续可靠检测次数、上次识别时间和复用次数；帧级保存最后完整检测时间、frameId 和来源/取消/版本标识。缓存最多保留一帧，RGB≤64 MiB、文字框≤4096，时序按配置最多复用 60 次/10 秒；旧精确缓存仍保持 30 帧/10 秒上限。

## 三条执行路径

- **Full**：首次、来源或尺寸变化、非递增 frameId、边缘/边界风险、低置信度、周期到期时完整检测和识别。两个完整检测必须在 2 px 内给出相同边界，才允许尝试 Rec-only。
- **Cache-only**：逐字框带 16 px 外围的 RGB 必须完全相同，才复用文字。整帧相同时可走快速比较；其他情况下还要扫描识别范围内每个像素，检查旧框外新增高频边缘。历史高置信度、低平均差异或相似形状都不会让变化的文字直接复用旧结果。
- **Rec-only**：边界已可信、没有框外/边界高频变化时，对当前图像的变化文字框重新透视裁剪与识别；其余精确相同的行可以复用。变字必须经过识别器。低置信度或空输出在发布前退回 Full，失败尝试的耗时和推理次数保留。

传统特征使用原分辨率灰度、水平/垂直有符号梯度、框内边缘数和框周围亮度。全范围检查没有缩略图采样遗漏小字的问题；平滑背景变化可以通过，但纹理运动、阴影位移、打字机越界和布局移动通常会触发 Full。没有实现自动移动四边形或无限期模板跟随，宁可重新检测。启发式不能证明范围外永无新字；低对比新增文字可能等到强制刷新，所以仍是实验功能。

默认刷新 1000 ms、最多 30 次复用、可信置信度 0.85、梯度变化阈值 12。刷新计时从完整结果完成开始；实际新字响应上限还要加捕获等待、OCR 调度和完整推理时间，不能声称严格 1 秒内出现。策略 Full 禁止复用；Rec-only 仍只作用于可信框，不跳过保护；Cache-only 在文字变化时回 Full；Auto 自动选择。

## 调度和生命周期

时序仅启用单 Worker、完整坐标范围；Single 直接使用，Manual/Adaptive 限制为单实例并说明原因。Adaptive 保留自身可用设备约束，不能借此启用未实现的 GPU Adaptive。旧缩略图变化门控在时序激活时被绕过，避免漏掉一个 HUD 数字。pause/stop、切换窗口、取消、模型卸载、重载和崩溃恢复都会通过 generation/epoch/revision 或 Session 生命周期使旧状态失效。GGUF 不使用此缓存。

`recognizer::recognize_regions` 是真正独立的内部入口，仅接收当前 RGB、四边形和识别 Session；不会隐式调用检测器。支持原坐标多框、透视变换、动态 batch 和现有宽度桶/像素预算；CPU 缓冲跨帧复用，padding 每次清零。`pipeline.detectorCalls/recognizerCalls` 用实际调用计数验证路径。

## 独立语料和标注

`scripts/create-temporal-fixtures.py` 生成 `tests/fixtures/temporal/corpus.json`，包括静态对话框、平滑动态背景、逐字显示、整句替换、单数字、新增/消失区域、滚动、移动/尺寸变化、半透明面板、中日混合、场景切换、generation 重置、渐隐、移动阴影、变化纹理背景、2K/4K。测试不是实际游戏录像。真实 WGC 的暂停/停止、崩溃/恢复由 `desktop-parallel.mjs` 的 `GAMEOCR_TEST_TEMPORAL=1` 分支另测，不能拿图像序列模拟结果代替生命周期验证。

真实序列入口：在浏览器打开 `scripts/temporal-annotator.html`，选择同一目录的截图，逐帧拖框、填写文字、设置时间/来源编号并确认校对。把导出的 `annotations.json` 放到图片目录，再执行：

```powershell
node scripts/import-temporal-corpus.mjs C:/capture/annotations.json .cache/real-temporal
$env:GAMEOCR_TEMPORAL_CORPUS='.cache/real-temporal/corpus.json'
$env:GAMEOCR_TEMPORAL_REPORT='docs/test-results/temporal-real-cpu.json'
node scripts/benchmark-temporal.mjs
```

导入验证标注和时间、计算图片 SHA-256，目标必须是新目录，不覆盖原始截图。没有提供真实序列时不会自动生成“真实游戏”标签。原生时序脚本按处理速度顺序送帧，`atMs` 是语料时间，不是实时回放时钟。

## 验证和结果口径

每个场景比较 Full、旧精确缓存、Temporal Cache-only、Rec-only、Auto，分别启动进程；保存加载、首帧、原生耗时均值/p50/p95/p99、实际调用、路径、GT CER/召回、输出差异、资源快照。新旧文字集合变化单独报告首个响应墙钟、漏掉的新字和残留旧字；即时响应之外的遗漏记为未恢复，不能记作 0 ms。静态重复帧的低中位数不代表内容变化帧同样快。

初轮 CPU 15 场景×5 策略均无相对完整 OCR 的文字差异。1280×720 三行对话框：静态 Auto 约 2 ms；平滑动态背景约 51 ms；半透明面板约 45 ms。2K/4K 静态原生 Auto 约 21/38 ms，未达到 5 ms。完整小字识别仍耗时数秒，Rec-only 识别全部 53 行也远高于 50 ms。CPU/CUDA 的逐帧原始报告保存在 `docs/test-results/temporal-*.json`，最终数值以相应报告为准。

仍需真实游戏序列检验复杂 3D 背景、抗锯齿变化、渐隐/阴影、小字号长文本和低对比新字；特征通过不等于识别绝对正确，输出一致也不等于完整 OCR 本身无误。默认不推广到所有场景。
