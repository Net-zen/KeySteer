# UI 扫描

链路：Mode 发 `ScanUi` → Backend 调度原生扫描 → 合并与去重 → 分批 `UiScanned` → Hint 会话更新。

## 入口

- 协议：`src/api/command.rs`；消费端：`src/modes/hint/`。
- Windows：`src/platform/windows/accessibility.rs`、`src/platform/windows/ui_scan.rs`、`src/platform/windows/vision/`。
- macOS：`src/platform/macos/accessibility.rs`、`src/platform/macos/ui_scan.rs`、`src/platform/macos/vision.rs`。
- 共享算法：`src/platform/common/scan_mailbox.rs`、`src/platform/common/scan_accumulator.rs`、`src/platform/common/spatial_index.rs`、`src/platform/common/contour/`。

## 必须保持

- 扫描异步、可按 scan id 取消，流式交付 Partial；Engine 不等待完整扫描。
- 结果验证 scan id 和目标上下文；旧请求不能覆盖新会话。
- 终态与数据分开理解：超时不等于清空已有目标；重扫和退出清理旧会话资源。
- 遍历、候选数量、图像和在途工作有明确边界；取消不能只取消显示而让旧任务无限运行。
- 去重及多来源融合保持稳定语义，不依赖哈希迭代顺序。
- UIA/AX 等原生引用由对应平台线程拥有，跨线程只交付允许传递的数据。
- OCR provider 共用 `UiTarget::recognized_text` 构建文字目标，以 `StaticText` 角色进入共享融合（包括 Windows 系统／微信 OCR、macOS Vision）；不要按长宽比改成链接或控件，否则会丢失 OCR 来源与搜索／复制元数据。
- OCR 接缝文本在公共 `ScanAccumulator` 中从原始片段按行和横坐标重建；仅在同行且位置重叠时消除首尾重复，无法对齐的识别内容保留。空格清理复用预览规则，拼接结果同时用于 OCR 元数据和文字目标名称；后续替换不得重新追加旧片段。首尾匹配采用有界线性算法（最多 512 字节），拼接字符串在扫描内复用，随 finish／会话释放清理，不进入按键热路径。

策略、范围、超时与目标上限查配置和代码；扫描算法可调整，但应覆盖取消、增量顺序、重复目标和跨屏场景。

`UiScanScope::resolve_window` 共用惰性优先级：Window 为鼠标窗口／激活窗口，Active 为相反顺序，两者最终以屏幕保底；Screen 不查询窗口。只在范围解析阶段回退，不因空结果或 provider 错误另扫其他范围。共享结果的 activate 标志仅对 Window 首选鼠标窗口置位，原生后端复用现有窗口激活原语，拒绝激活不重选范围。macOS 在 worker 用一次 Quartz 元数据选择窗口，AX 按该窗口的 PID／矩形查找 AXWindows 根，Vision 直接使用同一捕获范围，不独立查询 AXFocusedWindow。

ScanMailbox 在激活前保存当前 generation 的 UiScanActivationExpected；两端在原生焦点通知和结果前交付它。Engine 按 scan owner 转交，Hint 消费匹配进程的预期焦点事件而不启动第二次扫描；其他焦点变化照常重扫。取消／新 generation 清除未交付标记，macOS 扫描上下文允许本轮主动激活的目标 PID。
