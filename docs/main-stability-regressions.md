# main 稳定性修复与回归记录（2026-10-27—28）

本轮从实际 fetch 后的 `origin/main`：`b5f82e53c2091981cc6729ba1c73f8304f44024d` 开始，工作区初始干净；修复分支为 `fix/main-stability-regressions`。不更新依赖、替换模型或重写 Executor。原有基准 JSON 不覆盖，所有新证据放在 [stability-2026-10-27](test-results/stability-2026-10-27)。

## A. 七项修复状态

| 问题 | 开始时是否存在 / 根因 | 修改位置与方案 | 针对性回归 | 结果与限制 |
| --- | --- | --- | --- | --- |
| P1 GPU 分发 | 存在：公开 Releases 为空；生成 ZIP 不等于公网可下载。默认 tag 只由 OCR ZIP 决定，不能反映单独更新翻译 Worker。已有运行库校验规则本身可工作。 | `scripts/package-gpu-backends.ps1`：两份哈希共同决定默认 tag；`verify-gpu-backends.ps1`：验证独立 runtime/id、URL、文件名、SHA、大小、Worker 版本、许可证和内嵌清单；便携打包与 CI 调用该校验。`cuda_runtime.rs:846` 增加损坏 ZIP 清理测试。 | 本地 ZIP/清单正例，篡改 size/hash/url/kind/runtime/required/version、内嵌 size 负例；原有缺文件、哈希错误、原子安装、CPU-only 测试保留。 | 本地校验 PASS。两个最终 URL 真实 HEAD 均 404，公开下载/加载 SKIPPED，需人工发布匹配资源。未创建 Release/Tag 或上传二进制。 |
| P2 Profile 语言 | 存在：后端只取全局语言，UI 单次复制无法跟随编辑；Prompt 未约束源语言。 | `translation/config.rs:59,162`：有效语言按手动覆盖 → Profile → 全局解析；旧保存文件迁移保留手动差异；`TranslationSettings.svelte` 显示跟随/覆盖；`prompt.rs:99` 明确源语言，`text.rs` 保留 Auto 启发式及非文本跳过。 | 无 Profile、两个 Profile、编辑重载、旧配置迁移、手动覆盖、缓存 Key、提示词、纯汉字/混排/英文/数字与同语言跳过。 | 单元测试 PASS；修复前 Profile 和源语言 Prompt 测试实际失败，有日志。Auto 仍是启发式，纯汉字不直接判中文。 |
| P2 运行中取消 | 存在：只覆盖 pending/拒绝旧发布，native 生成仍跑完。 | `translation/mod.rs:70,229`：每个文本 revision 的取消标记；`ocr.rs:150`：传输取消后继续读取原请求响应；`protocol.rs:57,105`：独立控制读取线程、有界队列、按 request_id 取消；`runtime.rs:192,228`：prompt chunk/Token 边界检查，KV 清理与 sampler 释放。 | 确定性延迟引擎验证实际提前停止、同模型复用、取消/卸载/退出；Queue 测 A→B→C、同字不取消/不重置等待、generation/epoch 变化；真实 native 与桌面验证见下文。 | 单元测试 PASS。不能抢占单次 native decode；旧 Worker 无 supports_cancellation 时保持原来的丢弃过期结果行为，新的配套 Worker 支持协作取消。 |
| P2 不完整缓存 | 存在：截断结果进入 LRU，持久恢复不筛选。 | `cache.rs` 入口拒绝截断/空项；`types.rs:249` 和 manager 只缓存非空 eos 成功结果；batch 全部校验后插入；新增兼容默认的 `TranslationResult.stopReason`，截断状态 incomplete。 | 完整条目保留、历史坏项过滤、坏项不能替换好项、空项、batch 部分/截断拒绝；修复前旧截断缓存测试实际失败。 | 单元测试 PASS。无需手动清空缓存；Prompt 版本从 1 升至 2，旧 Key 保留但不复用于新提示词。 |
| P2 Auto 故障回退 | 存在：重建只使用已选中的 CUDA，失去 Auto 意图。 | `translation/worker.rs:13,39` 局部有界加载 Helper；Session 保留 preferred；`mod.rs:596,659,765` 初始/恢复共用，CUDA 崩溃后 Auto 下一次改 CPU，成功后更新真实设备并清除旧错误。 | 注入初次 GPU 成功/失败、双重失败、主动取消、CPU/显式 CUDA、运行故障后的设备选择、CPU 粘性恢复；实际 CPU 崩溃 E2E。 | 单元测试 PASS。最多 CUDA+CPU 两次加载尝试，不进入下载流程；显式 CUDA 保持错误。真实 CUDA 崩溃恢复不以模拟测试冒充硬件验收。 |
| P3 部分缓存合批 | 存在：任一命中即放弃整批，未命中项逐条推理；整批总数复制到每项。 | `batch.rs:13,52` 先固定 skip/hit 快照，再筛出 misses；`mod.rs:469` 仅批量 misses，验证失败只回退它们；总耗时/Token 只归属于批内首项，其余保留 batch ID。 | 全无/全有/3+3/仅一项未命中、LRU 驱逐、skip、Token/字节预算、ID 乱序/缺失/重复/未知、截断/部分 JSON、总数不重复；桌面真实 3+3 验证见下文。 | 单元测试 PASS。保留 2–8、512/2048 字节、每项 96 Token 限制，不兼容选项仍逐项处理；身份/原始几何发布检查保留。 |
| P3 Profile 版本 | 存在：UI 每次保存把所有 Profile 至少加一。 | 移除 UI 无条件递增，`translation/config.rs:85` 在后端比较术语、风格、背景和语言语义；未变化保持，导入高版本保留，新建至少 1，溢出明确失败。 | 原样/设备/等待不增，五类语义变化只增 A、不影响 B，高版本导入、再次保存稳定、拒绝降低版本；UI 保存回归见下文。 | 单元测试 PASS。名称/进程绑定不改变翻译语义，不递增。原 JSON 导入/编辑/导出保留。 |

行号以本轮修复提交为准。确定性测试使用的延迟引擎/失败注入只证明调度、协议和恢复策略，不作为模型推理或 GPU 性能证据。

## B. 已执行检查与证据

- **PASS** Rust：core 24、translation 15、ONNX 14、desktop lib 35，共 88 项；fmt 检查通过。[日志](test-results/stability-2026-10-27/desktop-unit.log)
- **PASS** `npm ci`、Svelte/TypeScript 检查（0 错误/警告）、Vitest 2 项、生产前端构建。[前端检查](test-results/stability-2026-10-27/frontend-check.log)
- **PASS** 本地 GPU 归档/内嵌清单验证及 8 类负例。[记录](test-results/stability-2026-10-27/gpu-validation-final.log)
- **PASS** 最终 NSIS、MSI 构建及安装包 EXE 内嵌 GPU 清单校验；标准便携包 59 个 ZIP 文件/47 个 x64 依赖、CPU-only 包 57 个文件/46 个依赖验证通过。两个便携包的实际桌面测试使用同一 EXE SHA `1a38dcb00284490234a48c7c3b0405b9505a4123b1ddb88a929650cab26e8b8b`；重建安装包的 EXE 单独记录，不把便携包测试当作安装测试。[最终产物大小、SHA 与验收范围](test-results/stability-2026-10-27/packaging.json)、[安装包构建](test-results/stability-2026-10-27/build-installers-verified.log)
- **PASS** 真实 Hy-MT2：CPU 14 条、低温度 CPU 14 条、CUDA 14 条、合批 1 条、单 Token 截断 14 条，首尾同句检查 KV 隔离。[CPU](test-results/stability-2026-10-27/translation-cpu.json)、[CUDA](test-results/stability-2026-10-27/translation-cuda.json)、[合批](test-results/stability-2026-10-27/translation-batch.json)、[截断](test-results/stability-2026-10-27/translation-output-limit.json)
- **PASS** 真实 CPU Worker 协作取消：长 prompt 处理中发送取消，返回 cancelled，然后同一已加载进程成功翻译下一句。本次请求到取消响应约 4.45 秒，主要等待单次 native prompt decode；不宣称能在 native 调用内部即时抢占，也不是延迟基准。[记录](test-results/stability-2026-10-27/translation-cancellation.json)
- **PASS** 真实原 OCR：ONNX small → GGUF Q8_0 → ONNX medium 生命周期/身份切换；PP-OCRv6 CPU 的 8 个识别案例及畸形 IPC、卸载重载、Worker 替换。[切换](test-results/stability-2026-10-27/runtime-switch.json)、[ONNX](test-results/stability-2026-10-27/onnx-cpu-e2e.json)
- **PASS** Windows Desktop 翻译 19 项、原 OCR 12 项：实际 WGC/PP-OCR/Hy-MT2、3 条命中 + 3 条新菜单仅 1 次调用、整批总数不重复、截断重试不命中、UI 语言覆盖/回归默认、Profile 版本稳定、当前帧关联、退出清理、持久缓存重启命中及 Worker 崩溃恢复。[翻译完整报告](test-results/stability-2026-10-27/translation-desktop.json)、[CPU 包 OCR](test-results/stability-2026-10-27/desktop-runtimes.json)
- **PASS（限定场景）** 10 月 27 日同一 EXE 的实际 OCR CPU + 翻译 CUDA、OCR CUDA + 翻译 CUDA 两种组合；该次运行随后因为测试的精确 label 定位超时而整体失败，修正定位后于 28 日完成上面的全量通过。保留失败边界，不把部分通过称为整次通过。[GPU 部分运行记录](test-results/stability-2026-10-27/desktop-gpu-partial-run.json)
- **SKIPPED** 28 日全量桌面测试中的 GPU 组合及新加的真实 Auto CUDA 崩溃回退：可用 1716 MiB，小于约 1657 + 128 MiB 的准入要求。显式 CUDA 错误、Auto 真实 CPU 回退通过；Auto CUDA 故障后的选择/重建/粘性策略由确定性注入测试覆盖，硬件故障路径仍待显存充足时运行。未降低准入或结束其他应用来强行通过。
- **FAIL（修复前，预期复现）** Profile 有效语言、源语言 Prompt、历史截断缓存测试；修复后已转 PASS。[Profile](test-results/stability-2026-10-27/before-profile.log)、[Prompt/缓存](test-results/stability-2026-10-27/before-translation.log)
- **SKIPPED** 公开 GPU 资源下载和首次联网加载：最终两份资源地址均 404。官方模型地址 HEAD 为 200、长度与固定清单一致；这不等于本轮完成 1.13 GB 网络下载和 Range 续传。[资源状态、URL、SHA、大小](test-results/stability-2026-10-27/resources.json)
- **SKIPPED** 远端 CI 成功验收：已检查的 main 两次运行均 cancelled，本地测试不替代远端 CI。[起始提交 CI](https://github.com/suhli/gameocr/actions/runs/40437072626)、[后续 main CI](https://github.com/suhli/gameocr/actions/runs/40443392824)
- **SKIPPED** 干净 Windows 的 NSIS/MSI 安装启动、真实 ADB 设备及长期游戏负载；本轮未使用对应验收环境。

## C. 兼容性

- OCR 配置 Schema、模型/Runtime/Executor、捕获实现、grounding/bbox/polygon/confidence、历史及旧导出字段未改。共享传输原 `request_typed` 调用不发送 cancel。
- 翻译设置仅新增可空 `sourceLanguageOverride` / `targetLanguageOverride`；旧文件迁移保留手动语言差异。Profile 版本由后端维护，旧 JSON 仍可导入导出。
- 缓存文件仍为 version 1，过滤截断/空项而非删除全部缓存；Prompt version 2 自然隔离旧 Key。无需重下模型，不变更量化、官方固定 revision 或模型 SHA。
- Worker NDJSON 版本仍为 1，增加 `cancel { request_id }` 和 Ready 的 `supports_cancellation` 能力位；老 Worker 能力位缺省 false，不会收到未知命令。取消不额外返回一条响应，原 Translate 返回 cancelled 终止结果。TranslationResult 新增默认空 `stopReason`。
- CPU-only 包仍不启用 GPU 下载；Auto 不下载，显式 CUDA 才准备组件。OCR CUDA 与翻译 CUDA 命名空间独立。Microsoft/NVIDIA 固定版本和校验规则未修改。
- `.gitattributes` 未改，没有新增需 LFS 的二进制资源；已有测试 PNG/模型实际可用。原始性能报告保留，新结果仅作为本轮稳定性证据。

## D. Git 与人工验收边界

起始 main：`b5f82e53c2091981cc6729ba1c73f8304f44024d`；分支：`fix/main-stability-regressions`。当前轮次的最终提交号和提交清单见最终交付回复。

实现提交：

- `2ea5614`：Profile 有效选项与版本、缓存完整性、协作取消及 Auto 加载恢复基础修复。
- `4bbeb2c`：部分缓存合批、指标归属、取消/恢复/队列回归与语言边界。
- `acd6470`：独立 GPU ZIP、tag、内嵌清单及打包负例校验。
- `394eeb7`：实际模型/桌面回归、独立报告输出及恢复后错误状态清理。

变更范围为翻译 crate、桌面翻译模块与有界传输、翻译设置/结果组件、GPU 打包和相应测试/文档。未修改原 OCR Executor、模型默认量化、依赖锁定版本、捕获实现或原性能基准 JSON。

外部剩余事项：发布资源状态文件中列出的两个精确 ZIP（最终 tag 为 `gpu-e51f20325a78`），公开后再验证匿名完整下载、SHA、缓存安装及加载；显存充足机器上的真实 Auto GPU 故障恢复；干净 Windows 的 NSIS/MSI 安装启动；真实 ADB 设备及长期真实游戏负载。本轮不将未执行项计为 PASS，也不创建公开 Release、Tag 或推送 main。
