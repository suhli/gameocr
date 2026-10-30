# 译文回填到预览与游戏窗口（2026-10-29）

后续新增游戏字幕本地调试日志并重新打包，最新说明与产物校验见 [本地诊断验证](translation-overlay-diagnostics.md)。以下保留本次回填修复的历史验证信息。

用户反馈翻译已完成但没有回填画面，并明确要求游戏窗口和 GameOCR 预览两处都显示。续作基线是 main 的 `adfb8d3b90a2b92a64bf14e02ce9b8d410257218`。

## 原因与修复

原实现仅在结果列表展示译文，CapturePreview 没有接收或绘制译文；独立游戏字幕默认关闭且入口藏在设置面板中。本机自动翻译已开，但尚未保存字幕设置，因而游戏字幕实际仍处于关闭状态。此外，原先开启字幕会清空状态，必须再来一次 OCR 才能显示，静态画面又可能被变化检测跳过。

- 预览增加独立译文层，复用已有定位和磨砂字幕组件；默认开启，缩放/适应窗口一起重新布局。按实际展示的截图匹配译文，拒绝错帧、错会话、错模型、过期版本、源文本不符和无效几何，避免把新译文贴到旧截图上。
- 主界面直接提供「预览译文」与「游戏字幕」两个开关。游戏字幕的显隐原因直接可见；设置面板保存后同步更新开关。仅切换游戏字幕不会调用翻译或卸载模型，也不会覆盖用户后来保存的样式。
- 开启游戏字幕时只复用后端当前 live job 的精确匹配结果；保留原观察时间，不延长过期内容的寿命。停止、暂停、模型/目标切换或无当前任务时不能通过历史恢复字幕。
- 本机字幕配置已保存 `overlayEnabled: true`，不影响其他模型/捕获设置。未中断正在运行的程序，需要启动新版后生效。

原始截图、OCR 输入及历史导出继续保持原数据。预览可以展示历史截图自己的译文，独立游戏字幕仍只展示当前有效结果。

## 验证

- **PASS** Svelte/TypeScript：0 错误、0 警告。
- **PASS** 前端单元测试 14 项，新增 5 项覆盖预览身份/文字/几何和乱序事件。
- **PASS** Rust 桌面库 44 项，新增“开启字幕复用当前结果、保留年龄、拒绝跨会话/停止后的历史”回归。
- **PASS（界面组件范围）** `test-overlay-display.mjs` 的 5 组无界面浏览器检查。实际 Svelte 组件显示中文，预览随缩放定位，开关和错帧清除生效，独立渲染器确认布局并拒绝延迟旧事件。输入为固定模拟数据，不是实际 OCR/翻译或 Windows Overlay 验收。
- **SKIPPED** 实际游戏窗口叠加、WGC 排除、OS 点击穿透、高 DPI/多显示器、安装器实际运行，沿用用户延后桌面验收的要求。本轮没有操作用户桌面。

- **PASS（构建/文件范围）** 前端生产构建、Release 程序、NSIS / MSI 安装器，以及 debug 程序/fixture 均已重新生成。CPU ZIP 的 57 个文件/46 个原生二进制与按需 ZIP 的 59 个文件/47 个原生二进制校验通过；按需包内嵌 GPU 清单与配套资源一致。未执行安装、卸载和实际桌面运行验收。

修复实现提交为 `9465308`（Show translated captions in preview and expose game subtitle control），分支仍为 main，未推送。原始日志与报告保存于 [证据目录](test-results/overlay-display-fix-2026-10-29/)：

- [Svelte 检查](test-results/overlay-display-fix-2026-10-29/frontend-check.txt)、[前端测试](test-results/overlay-display-fix-2026-10-29/frontend-tests.txt)、[Rust 测试](test-results/overlay-display-fix-2026-10-29/rust-tests.txt)。
- [组件渲染报告](test-results/overlay-display-fix-2026-10-29/display-report.json)、[安装器构建](test-results/overlay-display-fix-2026-10-29/installers-build.txt)。
- [产物路径、大小与 SHA-256](test-results/overlay-display-fix-2026-10-29/packaging.json)、[CPU 包校验](test-results/overlay-display-fix-2026-10-29/verify-cpu.txt)、[按需包校验](test-results/overlay-display-fix-2026-10-29/verify-ondemand.txt)。

界面测试截图在本机 `artifacts/overlay-display-fix/preview-translations.png` 和 `standalone-renderer.png`，未作为真实游戏截图。当前同名分发包已更新为本次修复，旧验证记录中的哈希不再代表最新本地产物。

## 使用

启动新版，加载 OCR 与翻译模型并开始捕获/识别。译文完成后会回填到「识别帧」预览；在主界面开启「游戏字幕」后，切回被捕获的游戏窗口，才允许独立字幕显示。若提示过期，请重新识别一次；查看应用主窗口时游戏已在后台，游戏字幕会按原策略隐藏，而预览译文仍可查看。
