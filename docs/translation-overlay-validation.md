# Windows Translation Overlay 验证记录（2026-10-29）

Overlay MVP 已接入现有实时 OCR / Translation 管线。桌面实际显示、点击穿透和 WGC 排除仍待验收，不能将本轮静态及原生模型检查视为完整功能验收。用户已明确要求先完成其余工作，将桌面验证留待之后。

## Git 与实现范围

- 功能开发基线：`7f67dbf05f68b6479a81a9de53c4b047674f76ce`，包含前一轮 main 稳定性修复。
- 按用户后续明确要求，继续在 `main` 开发，没有创建需求模板中的功能分支。本轮没有推送、打标签或发布 Release。
- 本次续作开始时工作区干净，已有实现提交 `0b01b4a2340be6a04afb1f299a26484e0f4fd1b6`。
- 定位修复提交：`be7edea3dbbdd15d0130f97f9386200b85682aa9`（Fix overlay anchors when following or switching target windows）。该提交是本轮应用产物的实现版本，后续提交仅包含测试脚本、证据和文档。
- 测试补强提交：`acab7e6`（Strengthen overlay desktop assertions and add layout microbenchmark）。最终文档/证据提交与最终 HEAD 在交付消息中给出；文件清单见 [相对功能基线的改动](test-results/overlay-2026-10-29/changes.txt)。
- `src-tauri/src/overlay.rs` / `overlay/state.rs`：独立窗口、配置、字幕状态、有效期和事件同步；`window.rs`：稳定目标身份和物理坐标映射；`service.rs` / `translation/mod.rs` / `commands.rs` / `lib.rs`：现有生命周期接入。
- `src/overlay/*`、`src/lib/overlayGeometry*`、`src/types/overlay.ts`：透明入口、轻量 keyed 字幕、混合文字布局；`OverlaySettings.svelte`、设置面板和 API 接入独立持久化。
- `scripts/desktop-overlay.mjs`、`overlay-native.ps1` 与 `capture-fixture.rs`：实际桌面验收入口；`benchmark-overlay-layout.mjs`：无需桌面权限的纯布局计时。
- 续作修复：固定位置锚点绑定稳定目标身份；跟随时更新锚点，关闭跟随后保留最近位置。快速切换目标不会沿用旧窗口的位置。增加相应状态测试，并删除测试辅助程序的全局 Alt 输入重试。桌面脚本的显隐/移动断言同时检查真实 HWND 和渲染 serial，关闭后的输入检查要求计数实际增加，不以后台快照或发送输入本身充当通过。

详细实现、坐标映射、设置和使用方法见 [功能说明](translation-overlay.md)。本轮没有修改模型、OCR Executor、推理依赖或模型缓存规则。

## 已完成的检查

- **PASS** TypeScript/Svelte：0 错误、0 警告。[日志](test-results/overlay-2026-10-29/frontend-check.txt)
- **PASS** Vitest：9 项（原几何 2 项，Overlay 7 项）。覆盖非整数/非等比例映射、负坐标裁剪、非法框、多块联合、混合文本边界、样式范围、重定位和旧快照拒绝。[日志](test-results/overlay-2026-10-29/frontend-tests.txt)
- **PASS** 前端生产构建：Overlay 独立入口和样式分包，未加载完整主界面。[日志](test-results/overlay-2026-10-29/frontend-build.txt)
- **PASS** Rust：96 项（桌面 43、核心 24、ONNX 14、翻译 15）。字幕状态覆盖旧配置持久化、几何拒绝、相同文本十帧保留/重定位、新文本失效、身份/模型/目标变化、完整译文过滤、严格时限、前台/可见条件和重复开关。窗口系统实际行为仍待桌面验证。[日志](test-results/overlay-2026-10-29/rust-tests.txt)
- **PASS** Rust 格式检查。首次受限执行中的临时文件/本机服务访问失败已使用所需权限重跑，未通过删除测试或改动断言绕过。
- **PASS** 原 OCR 原生回归：PP-OCRv6 small/medium 的 8 个 CPU 图像案例，以及畸形 IPC、卸载重载和 Worker 替换；ONNX small → GGUF Q8_0 → ONNX medium 的真实模型生命周期/身份检查。输入为既有合成测试图，非真实游戏、非 WGC 桌面回归。[ONNX](test-results/overlay-2026-10-29/onnx-cpu-e2e.json)、[Runtime 切换](test-results/overlay-2026-10-29/runtime-switch.json)
- **PASS** 后续桌面验收所需 debug 应用和 Win32 fixture 编译，未运行窗口。[日志](test-results/overlay-2026-10-29/debug-fixture-build.txt)
- **PASS（构建/静态范围）** NSIS / MSI 两种安装器生成；CPU 便携包 57 个 ZIP 文件/46 个 x64 文件、按需包 59 个 ZIP 文件/47 个 x64 文件校验通过。逐项比对 ZIP 与目录文件哈希，并检查原生依赖；CPU 包不带 GPU 组件，按需包不带 ONNX GPU DLL 或 llama.cpp/翻译 CUDA Worker。不是安装/运行验收。[安装器日志](test-results/overlay-2026-10-29/installers-build.txt)、[CPU 校验](test-results/overlay-2026-10-29/verify-cpu.txt)、[按需包校验](test-results/overlay-2026-10-29/verify-ondemand.txt)
- **PASS（离线范围）** 两份配套 GPU ZIP、清单和 EXE 内嵌清单一致，篡改体积、SHA、URL、Runtime 等负例被拒绝。GPU ZIP 哈希与上一轮相同。本轮未发布配套资源，未重新执行公网下载或 CUDA 模型推理，不能据此宣布公网分发完成。[日志](test-results/overlay-2026-10-29/gpu-offline.txt)、[内嵌清单检查](test-results/overlay-2026-10-29/package-ondemand.txt)
- **PASS** 真实 Hy-MT2 CPU 原生回归：默认参数 14 条、低温度 14 条、结构合批 1 条、单 Token 截断 14 条；首尾同句验证 KV 隔离。真实 Worker 处理中取消，然后复用同一进程/模型成功翻译下一句。本次取消响应约 **4.86 秒**，需等待 native decode 边界，不是即时抢占承诺。使用合成游戏文字语料，未验证真实游戏或本轮 CUDA 推理。[执行日志](test-results/overlay-2026-10-29/native-translation.txt)、[默认参数](test-results/overlay-2026-10-29/translation-cpu.json)、[低温度](test-results/overlay-2026-10-29/translation-cpu-low-temperature.json)、[合批](test-results/overlay-2026-10-29/translation-batch.json)、[截断](test-results/overlay-2026-10-29/translation-output-limit.json)、[取消及后续请求](test-results/overlay-2026-10-29/translation-cancellation.json)

## 本地产物

产物均位于仓库的 `artifacts/`，未上传或发布。完整 SHA-256、应用实现版本、Worker 哈希和校验范围保存在 [产物记录](test-results/overlay-2026-10-29/packaging.json)。

- `GameOCR-windows-x64-ondemand-portable.zip`：20,937,509 字节。
- `GameOCR-windows-x64-cpu-portable.zip`：20,225,872 字节。
- `GameOCR-windows-x64-setup.exe`（NSIS）：13,132,013 字节。
- `GameOCR-windows-x64.msi`：18,153,472 字节。

便携包使用最终 Release 输出，安装器各自带有 Tauri bundle 类型信息；未提取安装器内容来声称 EXE 字节完全相同。新重建的 CPU 翻译 Worker 与真实模型测试使用同一哈希。安装包未签名，依赖已有系统 WebView2；未运行安装/卸载程序。

## 纯布局性能

Windows 11 / Ryzen 5 5600G / Node 24.22.0，分别对 1080p、4K 的 3/32/64 条混合中日英字幕预热 1,000 次，再运行 10,000 次。4K / 64 条的平均布局耗时 **0.0265 ms**，P95 **0.0449 ms**；1080p / 64 条平均 **0.0258 ms**。这是单次本机、合成输入的纯坐标和布局微基准，不能换算为帧率影响或 WebView 更新延迟，也不代表高 DPI 已实测。[原始结果](test-results/overlay-2026-10-29/layout-benchmark.json)、[环境](test-results/overlay-2026-10-29/environment.json)

## 延后的验收

- **SKIPPED** Windows Overlay 桌面集成：字幕实际透明显示、不抢焦点、鼠标点击/拖动/滚轮穿透、窗口移动/缩放/最小化/关闭、同文无闪烁、取消/停止/切换隐藏、退出无残留。之前尝试未能将测试窗口置于前台，桌面访问授权超时；本轮遵照用户选择未重试操作桌面。
- **SKIPPED** 真实 WGC 前后截图对比。`WDA_EXCLUDEFROMCAPTURE` 设置及读回检查已实现，但不能据此宣布捕获排除通过。不支持该标志或设置失败时停用字幕窗口，原 OCR/翻译继续运行。
- **SKIPPED** 原 OCR / Translation 桌面回归，本轮不将前一轮稳定性测试结果计为当前产物通过。历史报告仍保留在 [稳定性记录](main-stability-regressions.md)。
- **SKIPPED** 高 DPI、多显示器、真实游戏、干净 Windows 安装/卸载。负坐标和非整数比例仅有确定性几何测试。尚未排除实际窗口中的定位误差。
- **SKIPPED** 实际事件到绘制延迟、DOM 更新频率、空闲 CPU、额外内存及连续 OCR 肉眼闪烁检查。桌面脚本提供采样入口；没有测量值时不声称开销达标。

## 视觉和使用边界

本版是深色半透明渐变、细边框、圆角和柔和阴影构成的磨砂视觉；CSS blur 限于 WebView 自身，不是真实跨窗口背景模糊。没有截图采样、背景修复、游戏注入或图形 Hook。默认关闭，只支持 Windows 普通窗口/无边框窗口；ADB 与独占全屏不在本轮范围。

源码要求只有当前前台、可见、稳定身份一致且几何匹配时显示。新文本立即清除，相同文本的新帧保留并重定位；默认 15 秒无新 OCR 到期，即使关闭提前隐藏仍有 30 秒硬上限。静态画面因变化检测长期跳过 OCR 时字幕可能到期，这是保守策略。

后续实际验收先运行 `node scripts/desktop-overlay.mjs`，再在真实游戏、混合 DPI 多显示器与干净 Windows 上复核。该脚本会切换测试窗口并发送鼠标输入，应在用户方便交互桌面时执行。测试窗口不能替代真实游戏验收。
