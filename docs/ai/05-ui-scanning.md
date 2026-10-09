# UI 扫描

链路：Mode 发 `ScanUi` → Backend 调度原生扫描 → 按来源提交就绪数据 → 复用的单写者融合线程 → 合并结果邮箱 → `UiScanned` → Hint 会话更新。

## 入口

- 协议：`src/api/command.rs`；消费端：`src/modes/hint/`。
- Windows：`src/platform/windows/accessibility.rs`、`src/platform/windows/ui_scan.rs`、`src/platform/windows/vision/`。
- macOS：`src/platform/macos/accessibility.rs`、`src/platform/macos/ui_scan.rs`、`src/platform/macos/vision.rs`。
- 共享调度与算法：`src/platform/common/scan_fusion.rs`、`src/platform/common/scan_mailbox.rs`、`src/platform/common/scan_accumulator.rs`、`src/platform/common/spatial_index.rs`、`src/platform/common/contour/`。

## 必须保持

- 扫描异步、可按 scan id 取消，流式交付 Partial；Engine 不等待完整扫描。
- 结果验证 scan id 和目标上下文；旧请求不能覆盖新会话。
- 终态与数据分开理解：超时不等于清空已有目标；重扫和退出清理旧会话资源。
- 遍历、候选数量、图像和在途工作有明确边界；取消不能只取消显示而让旧任务无限运行。
- 去重及多来源融合保持稳定语义，不依赖哈希迭代顺序。
- UIA/AX 等原生引用由对应平台线程拥有，跨线程只交付允许传递的数据。
- OCR provider 共用 `UiTarget::recognized_text` 构建文字目标，以 `StaticText` 角色进入共享融合（包括 Windows 系统／微信 OCR、macOS Vision）；不要按长宽比改成链接或控件，否则会丢失 OCR 来源与搜索／复制元数据。
- OCR 原始证据在扫描内维护空间索引，描述更新只检查可能相交的片段，避免小份额反复遍历全部已有文字。OCR 接缝文本在公共 `ScanAccumulator` 中从原始片段按行和横坐标重建；仅在同行且位置重叠时消除首尾重复，无法对齐的识别内容保留。空格清理复用预览规则，拼接结果同时用于 OCR 元数据和文字目标名称；后续替换不得重新追加旧片段。首尾匹配采用有界线性算法（最多 512 字节），拼接字符串在扫描内复用，随 finish／会话释放清理，不进入按键热路径。

策略、范围、超时与目标上限查配置和代码；扫描算法可调整，但应覆盖取消、增量顺序、重复目标和跨屏场景。

Windows 视觉 provider 超过退出期限时保留在隔离队列，仍未退出期间不启动新的视觉任务（包括此前已排队的扫描）。视觉来源释放截图隐藏门并返回 Unsupported 终态，Hybrid 的 UIA 来源继续交付结果；隔离线程全部结束并被非阻塞回收后，下一次扫描自动恢复视觉来源，不使用永久禁用标志。

`UiScanScope::resolve_window` 共用惰性优先级：Window 为鼠标窗口／激活窗口，Active 为相反顺序，两者最终以屏幕保底；Screen 不查询窗口。只在范围解析阶段回退，不因空结果或 provider 错误另扫其他范围。共享结果的 activate 标志仅对 Window 首选鼠标窗口置位，原生后端复用现有窗口激活原语，拒绝激活不重选范围。macOS 在 worker 用一次 Quartz 元数据选择窗口，AX 按该窗口的 PID／矩形查找 AXWindows 根，Vision 直接使用同一捕获范围，不独立查询 AXFocusedWindow。

ScanMailbox 在激活前保存当前 generation 的 UiScanActivationExpected；两端在原生焦点通知和结果前交付它。Engine 按 scan owner 转交，Hint 消费匹配进程的预期焦点事件而不启动第二次扫描；其他焦点变化照常重扫。取消／新 generation 清除未交付标记，macOS 扫描上下文允许本轮主动激活的目标 PID。

## 响应优先的调度

原生来源只移动已经可用的 Rust 数据，不执行公共融合、不等待其他来源或攒满固定条数。UIA／AX 遍历和 Windows 系统 OCR 转换逐项提交；已经完整的视觉候选直接移动整份 buffer。Windows 视觉数据绕过截图／OCR 协调器，其终态仍由协调器负责期限、取消和原生资源清理。

每个 Backend 按需创建一个融合 worker，后续扫描复用，关闭时按现有总期限停止。每轮持有独立 generation inbox；来源各自累计最多 `MAX_UI_SCAN_TARGETS` 个原始候选，空提交直接忽略，超额保留有效前缀，不产生容量等待。消费者轮转就绪来源，在共享锁内只移动当前份额，锁外完成可见范围验证、去重、OCR 描述和重叠融合。一个来源的数据多、空结果或迟迟未完成都不决定另一个来源何时发布。

融合采用较小的首份额及有上限的软 CPU 预算，自适应下一次处理数量；已有尾部立即处理，不使用填满条件或定时器。时间目标只用于按已测计算成本估算下一份数量；处理完成即尝试发布并继续，绝不等待补满时间目标。此份额与 `INLINE_LABELS` 的内联存储容量无关，常量和实际测量以源码及性能工具为准。每次份额后重新检查新 generation、取消和上下文；只在发布前执行原生上下文验证，预算仅计融合计算，避免慢原生查询导致份额缩小并倍增查询次数。成功终态排在已提交数据之后，ContextChanged 优先结束；退休、替换及描述更新保持同一可观察事务。输出继续使用 ScanMailbox 合并未消费增量，仅空到就绪时唤醒 Engine，并沿用原生输入／显示帧优先级。
