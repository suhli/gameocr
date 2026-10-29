# Windows 实时翻译字幕

在设置面板的「游戏翻译字幕」中开启，点击「应用字幕设置」，再切回当前捕获的游戏窗口。默认关闭。需要已有 OCR 模型、翻译模型、运行中的 Windows 窗口捕获及当前有效译文。支持普通窗口、无边框窗口；不支持 ADB Overlay，也不承诺独占全屏。

## 窗口与显示

独立的 Tauri `overlay` WebView 使用同一前端入口的 `?overlay` 分支，只加载字幕组件，不初始化主界面、历史面板或模型。无边框、透明、置顶、不出现在任务栏，使用 Tauri 的 `set_ignore_cursor_events` 配合原生 `WS_EX_NOACTIVATE`；启动时校验 `WS_EX_TRANSPARENT` / `WS_EX_LAYERED`，显示使用 `SW_SHOWNOACTIVATE`。

每 50 ms 在启用状态检查目标，关闭时退到 500 ms；新 OCR、译文、失效和设置事件会提前唤醒检查。只有显示数据或窗口几何发生变化才发出新快照、调整窗口。目标必须同时匹配 HWND、PID、进程创建时间、窗口销毁 epoch 和窗口类，且是当前前台可见窗口。移入后台、最小化、关闭或映射不确定时隐藏。重新获得焦点只恢复仍在有效期内的当前译文。

关闭「跟随游戏窗口」后保留当前目标最近的定位基准，目标移出该位置时隐藏字幕，防止字幕停在桌面其他应用之上。切换捕获目标会建立新基准，不沿用旧目标的位置。关闭字幕可重复开启同一个窗口；退出主程序会显式关闭字幕窗口和 Worker。

## 坐标与排版

OCR 坐标保持原捕获图像的 xyxy / polygon。后端在每显示时核对 WGC 图像尺寸与物理 DWM 可见边界、客户区、Win32 窗口矩形；优先匹配 DWM，匹配失败不猜偏移。客户区位置、DPI 和映射来源随轻量快照提供。窗口缩放期间旧图像尺寸不再对应当前窗口时，先隐藏，等新 OCR 后恢复。

原生窗口使用物理屏幕坐标，可包含负数；前端用实际 WebView CSS 尺寸分别计算 X/Y 比例，避免将 DPI 缩放应用两遍。布局支持浮点、非等比例变换，最终位置仍为 CSS 浮点像素。

多 Block 译文沿用原有分组与译文，保留来源 ID。坐标先裁剪到图像范围，再求联合框；缺失有效几何、超过 32 块、联合面积超过独立面积的 4 倍或占图像 45% 以上的多块组拒绝显示。单次最多展示 64 项。不会将距离很远的元素组成大字幕板。

字号默认 16 px，必要时缩小到 12 px。字幕宽度默认不超过画面的 45%，高度不超过 28% / 200 px，最多 6 行。中文自然换行，长英文单词可折行；超限使用行数截断。不够空间时尝试在文字框上下有限调整，仍与其他字幕重叠则省略该项，不无限扩张。

## 实时状态与失效

`OverlayManager` 不调用推理、不维护翻译调度。`TranslationManager::submit` 在原有有界队列内同步提供当前 OCR 和 Identity，`process` 通过原有发布校验后提供译文，`invalidate` 同步清空字幕。OCR 错误同样清空。主界面的历史浏览和导出不影响字幕。

- 新文本 / generation / epoch / revision / OCR 模型 / 目标变化：立即失效；过期事件不可恢复。
- 相同文本新帧：保留译文及 keyed DOM，按当前 Block 索引更新来源 ID 和几何。
- 完整、非空、无错误、未截断且确实经过翻译的结果才显示。未完成、取消、数字/同语言跳过和缺少几何不显示。
- 空 OCR 帧立即清空，不使用漏检宽限；优先避免残留。
- 停止/暂停 OCR、停止捕获、模型卸载、关闭 Overlay：清空。
- 无新 OCR 观察默认最多保留 15 秒，可设置 2–30 秒；关闭提前隐藏也仍有 30 秒硬上限。变化检测若长期跳过静态画面，字幕也可能因缺少新确认而到期，这是保守策略。

后端快照使用递增 serial；视图拒绝乱序快照，并在完成布局后确认 serial。原生 UI 线程再次核对 serial 后才显示，避免排队中的旧显示请求越过停止/失效。快照仅包含身份、文字、原图尺寸、几何及样式，不传截图 Base64 或历史数据。

## 捕获排除和磨砂边界

只对本应用创建的 Overlay HWND 设置 `WDA_EXCLUDEFROMCAPTURE`，检查设置结果和读回值；失败则停止显示该字幕窗口并提示原因，OCR 与翻译继续工作。WGC 保持现有窗口捕获及 `SecondaryWindowSettings::Exclude`。API 成功只能证明设置已请求，是否排除仍须以实际 WGC 对照截图验证。

Microsoft 对 [SetWindowDisplayAffinity](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity) 的说明将该标志限定于本进程顶层窗口，并从 Windows 10 2004 起支持。物理边界处理参考 [GetWindowRect 与 DWM 边界区别](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowrect)。

本版采用深色半透明底、轻渐变、细边框、圆角与柔和阴影模拟磨砂。`backdrop-filter` 仅作用于 WebView 内部合成内容；**没有实现或宣称真实跨窗口游戏背景模糊**。视觉强度同时调整轻渐变；不采样截图、不做背景擦除、修复或游戏注入。

## 配置与验证

配置使用现有 `data_dir` 和 `atomic_json` 机制，独立保存到 `overlay/settings.json`，缺失字段使用默认值，非法值拒绝保存。独立文件避免调用 OCR/Translation 设置更新路径而影响模型、缓存或推理。旧 OCR/Translation 设置无需迁移；损坏的 Overlay 配置保留原文件并关闭字幕回退安全默认值。

可重复检查：`npm run check`、`npm test`、`npm run build`、`cargo test -p gameocr --lib --locked`。`node scripts/desktop-overlay.mjs` 默认运行 debug 应用与 Vite，需要先构建 `gameocr` 和启用 `test-fixture` 的 `capture-fixture`，并准备 `.cache/test-models`、`.cache/translation/Hy-MT2-1.8B-Q4_K_M.gguf`。设置 `GAMEOCR_APP` 可验证发行包。设置 `GAMEOCR_OVERLAY_OUTPUT` / `GAMEOCR_OVERLAY_REPORT` 可选择新的证据路径，不覆盖原基准。

桌面测试使用真实 Win32 测试窗口、WGC、PP-OCR、Hy-MT2，包含系统鼠标点击/滚轮及前台切换。需要可交互桌面；辅助脚本只操作已核对 PID 的本次测试窗口。测试窗口证据不能替代真实游戏、高 DPI 多显示器或干净 Windows 安装验收。

纯坐标与混合文字布局计时：`node scripts/benchmark-overlay-layout.mjs artifacts/overlay-layout.json`，使用支持直接加载 TypeScript 的 Node 运行；不打开桌面窗口，也不代表 WebView 绘制或端到端延迟。本轮完成情况与未验收项目见 [验证记录](translation-overlay-validation.md)。
