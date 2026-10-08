# 平台后端

`src/platform/mod.rs` 选择实现；Windows/macOS 实现 `api::Backend`，`src/platform/unsupported.rs` 保持公共层可编译。共享协调逻辑放在 `src/platform/common/`，原生调用留在对应 OS 目录。

| 能力 | 两端入口（相对于 OS 目录） |
| --- | --- |
| 输入捕获与注入 | `hook.rs`、`input.rs` |
| 覆盖层与显示时钟 | `overlay.rs`；Windows 另有 `overlay_worker.rs`、`frame_clock.rs`，macOS 为 `display_link.rs` |
| 状态菜单与登录启动 | `status_item.rs`、`autostart.rs` |
| 辅助功能与视觉扫描 | `accessibility.rs`、`ui_scan.rs`、`vision` 相关实现 |
| 窗口与音频 | `window_manager`、`window_tabs`、`window_audio` 相关实现 |
| 原生资源封装 | `native/` 及各原生 owner |

## 边界

- 用 RAII 和借用表达资源寿命；unsafe 限于必要的原生边界，注明前置条件。不能为跨线程方便添加不成立的 Send/Sync。
- Windows 保持 Hook、COM、窗口与消息泵的线程归属；macOS 保持 AppKit 主线程、AX 引用和自动释放池的要求。
- 共享 worker 只持有可移植协议，原生身份在执行时校验；取消和关闭有边界，清理失败不能跳过其他资源释放。
- 原生回调避免慢 I/O；后台完成经事件端口唤醒 Engine，按 [运行时约束](02-runtime-and-api.md) 交付。
- 覆盖层不抢焦点且允许点击穿透；坐标和 DPI/Retina 转换由平台适配器负责。
- 权限、签名、焦点、系统全屏等行为需要目标 OS 实机验证，交叉编译不能替代。

原生错误统一进入 `src/support/logging.rs`；不得另建平台日志出口。

macOS 的 `native::OwnedCf` 只持有非空的 Create／Copy 引用；AX 查询失败或无结果返回空指针时，不创建临时 owner，也不调用 `CFRelease(NULL)`。鼠标窗口识别先走 AX 取点；失败或无法沿关系找到窗口时，才按 Quartz 前后顺序确定鼠标下最上层窗口，并复用编号枚举的 AXWindows 读取，只查询该进程、匹配唯一的普通且非最小化 AX 窗口。回退忽略本进程覆盖层，菜单／浮动层阻止选择后方窗口，不转向活动应用，也不猜测无法区分的重合身份；使用与编号枚举相同的 AX 超时和有限截止时间。正常取点成功不增加枚举。两条路径都失败时由现有 worker 回传结果，Window 模式保留窗口列表和编号选择，不退出进程。

Windows/macOS 的连续滚动共用 `common/scroll_worker.rs`：待执行邮箱由空变为有帧时立即唤醒独立执行线程；已有待执行帧时只替换，不重复通知。不等待上一帧完成、不使用周期计时器，也不与鼠标移动合并。执行器只保留最新未执行帧；原生调用变慢时替换过期帧，不累加距离或补播历史帧，优先响应当前输入。短按仍走各平台原有离散输入路径，Windows 连续帧不占 Hook 的有界注入队列。原生调用在邮箱锁外执行，失败返回 `InputInjectionFailed`；关闭先取消待执行帧，再按共享截止时间回收线程。

公共 `api::scroll::ScrollSession` 为帧提供可取消的代号；模式松键、换向、改速和退出时使旧帧失效，执行器注入前复核。已经进入原生 API 的一次调用可能完成；不得等待它来响应松键。每帧仅克隆共享身份，不新建会话或线程。

## 状态菜单与 About

macOS 使用 AppKit 的 NSMenu 和非模态标准 About 面板，由系统决定外观，不覆盖系统材质。`event_loop.rs` 在主线程派发 AppKit 事件，注册 common、event-tracking 和 modal-panel 模式的 observer 与可复用 deadline timer；原生菜单的嵌套循环期间仍驱动同一个 Engine。普通输入、异步完成和显示帧唤醒原生循环，期限按引擎定时任务及待确认窗口移动调整，不增加固定高频轮询。`display_link.rs` 在相同模式注册显示帧。

通用回合的有界批处理、事件路由、定时任务和退出清理只存在于 `app/runtime`，两种驱动都复用它。`event_loop.rs` 只拥有 AppKit/CFRunLoop 适配，并集中原来分散在 workspace/display-link 的原生唤醒、等待和事件派发；`workspace.rs` 只缓存焦点与外观。Windows 托盘线程无需这套嵌套循环适配，不能把 AppKit 注册或原生回调寿命搬到公共层。

每个回合有事件数量上限；native-driven poll 不派发 AppKit 事件、不等待，避免持有 Engine/Backend 借用时进入菜单循环。回调通过 RefCell 拒绝重入，在 FFI 边界收束 Rust panic。退出／错误先取消状态菜单跟踪并退出原生循环，注销 observer/timer 后才统一关闭后端资源；注册对象不得比回调上下文活得更久。模式仍遵循原有焦点、点击和完成规则，不因菜单开闭额外进入 idle。

Windows 使用独立托盘线程上的原生菜单和独立对话框线程上的 TaskDialog，保留系统交互与可访问性，不引入 WinUI/WebView 运行时。Win32 控件不承诺自动获得 WinUI 样式或玻璃材质。

窗口树平铺通过 `ApplyLayout.best_effort` 启用公共逐窗口容错：准备阶段跳过已知不可调整／全屏／失效窗口，提交或确认失败只跳过对应窗口，保留其他窗口的成功结果。`Applied.skipped_windows` 回传本轮失败身份，但不删除当前或历史分区关联，也不写永久应用黑名单；拒绝某个矩形不代表窗口永久不可调整。失败反馈本身不触发自动重试，后续分区编辑重新提交目标；编辑事务中的浮动窗口可用窗口编号再选分区重新加入。容错成功也回传查询／观察到的最小尺寸，供公共树算法约束后续边界调整；未变化或仍最大化的尺寸不作为新的最小尺寸证据。只对失败窗口重新查询约束，成功路径不增加原生查询。同步和异步路径共用该语义，取消仍恢复已尝试的操作，非法布局仍整体拒绝，Quick 的严格失败恢复不变。异步确认保持至多 16 个在途窗口与原有截止时间，完成后释放临时记录。部分成功的跳过信息仅作为界面状态，不输出 ERROR；公共模式仅显示窗口编号（最多三个，超出显示剩余数量，失效编号显示 `#?`），不为提示查询原生窗口。整体失败和恢复失败仍保留错误日志。

## 文本输入

UIHint 搜索由共享 Mode 编辑状态、presentation 绘制文字／选区／光标、runtime 拦截按键。两端只提供键盘布局字符与剪贴板，不创建搜索原生控件、不切换窗口焦点。字符捕获仅在搜索会话开启，退出／失败恢复时关闭。当前不提供 IME 组词接口，中文可通过简拼或粘贴输入。

窗口备注仍复用原生文本编辑器；调用 AppKit 的显示、关闭、文字设置或焦点方法前必须释放 RefCell 借用，避免同步回调重入。
macOS 应用重新激活时保留已有 field editor；输入法切换中的临时 first-responder 拒绝不能关闭备注面板。初次打开仍检查焦点是否成功建立。

多窗口连续操作由公共 worker 按目标保留小数移动余量和尺寸约束缓存，并在同一手势中合并交错目标的排队增量；撤销记录合并该手势的全部目标。音频批量请求在窗口 worker 解析全部选中窗口及 tab 成员的进程身份，独立 audio worker 按进程身份去重并逐项执行，单项失败不阻断其余目标。多选关闭 tab 组请求关闭全部成员，保存确认仍由应用处理。

## 点位取样

`api/point_sample.rs` 定义一个像素的异步请求／结果；runtime 按 owner 和版本路由，结束清除路由及原生取样状态。Windows 复用 overlay worker 的最新请求槽和 1×1 GDI 缓冲，在自有窗口 region 留一个三像素孔，再等待一次 DWM 提交，避免读取标签／标记且不隐藏整层；取消恢复 region 并释放像素缓冲。macOS 使用 common/point_sample 的单执行／单待办 worker，ScreenCaptureKit 按显示缓存排除本进程的 filter，使用 filter 的 pointPixelScale 将固定全局点映射到最多 32×32 像素的小区域，按原尺寸捕获并裁出指定的单个像素转为 sRGB；返回尺寸不符时拒绝样本，不将整张截图缩小成一个颜色；原生等待有界，超时后的回调自持有资源并释放迟到图像，屏幕捕获许可直到回调完成才释放。退出时取消、停止并有界 join，原生资源不进入模式。

搜索输入与 Point 显示共用固定文字基线和左对齐布局；`OverlayLabel::scroll_to_cursor` 将完整单行文字交给后端，用已有字形位置保持光标可见，非编辑态显示末尾。`trailing_text_len` 标出右对齐的计数后缀，两段文字共用一次布局，按实际计数宽度裁剪查询；查询裁剪还止于自身文字末尾，避免短输入重复绘制后缀。计数宽度及文字区边界在布局变化时计算并缓存，后缀使用 matched_text_color。Windows 复用 DirectWrite 编辑布局或 GDI advance 缓冲；macOS 复用 CoreText 测量和已有裁剪文字子层，光标／选区跟随同一水平偏移。Windows 输入视口使用不随下伸字符变化的固定偏移，空输入也按对齐方式计算插入位置；macOS 固定面板继续按字体行高居中。
