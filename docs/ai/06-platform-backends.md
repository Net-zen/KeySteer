# 平台后端

`src/platform/mod.rs` 选择实现；Windows/macOS 实现 `api::Backend`，`src/platform/unsupported.rs` 保持公共层可编译。共享协调逻辑放在 `src/platform/common/`，原生调用留在对应 OS 目录。

| 能力 | 两端入口（相对于 OS 目录） |
| --- | --- |
| 输入捕获与注入 | `input/mod.rs`、`input/hook.rs`；macOS 输入状态辅助模块也位于 `input/` |
| 显示、覆盖层与帧时钟 | `display/mod.rs`、`display/overlay.rs`；Windows 另有 `display/gpu_overlay.rs`、`display/overlay_worker.rs`、`display/frame_clock.rs`，macOS 为 `display/display_link.rs` |
| 状态菜单与登录启动 | `status_item.rs`、`autostart.rs` |
| 辅助功能与视觉扫描 | Windows `accessibility.rs`、macOS `accessibility/`；两端 `ui_scan.rs`、`vision/`；macOS 扫描来源协调仍在 `scan_workers.rs` |
| 窗口与音频 | `window_manager`、`window_tabs`、`window_audio` 相关实现；Windows 音频策略仅供 `window_audio/` 内部使用 |
| 原生资源封装 | `native/` 及各原生 owner |

## 边界

- 用 RAII 和借用表达资源寿命；unsafe 限于必要的原生边界，注明前置条件。不能为跨线程方便添加不成立的 Send/Sync。
- Windows 保持 Hook、COM、窗口与消息泵的线程归属；macOS 保持 AppKit 主线程、AX 引用和自动释放池的要求。
- 共享 worker 只持有可移植协议，原生身份在执行时校验；取消和关闭有边界，清理失败不能跳过其他资源释放。
- 原生回调避免慢 I/O；后台完成经事件端口唤醒 Engine，按 [运行时约束](02-runtime-and-api.md) 交付。
- 覆盖层不抢焦点且允许点击穿透；坐标和 DPI/Retina 转换由平台适配器负责。
- 权限、签名、焦点、系统全屏等行为需要目标 OS 实机验证，交叉编译不能替代。

原生错误统一进入 `src/support/logging.rs`；不得另建平台日志出口。

Windows `system_events.rs` 集中前台变化及控制台退出通知，两个注册对象仍独立管理回调和关闭状态。`native/gdi.rs` 同时保存桌面 DC、像素表面及复用它们的 `PreparedCapture`；截图类型和方法继续禁止 unsafe，线程归属及释放顺序不变。公共窗口历史的快照、提交和撤销／重做确认集中在 `common/window_session/history.rs`，取消及失败后的逆向记录仍由同一历史事务持有。布局／历史事务的分派枚举随 `window_session/worker.rs` 保存，原有串行屏障和独立确认实现保留。

能力目录只组织现有实现，不增加代理或资源管理层。`input/`、`display/` 的入口直接保留映射／注入和显示枚举实现；后端仍按原顺序创建与关闭资源。Windows 的 WeChat OCR 进程实现位于 `vision/wechat_ocr.rs`；macOS 的 Vision 桥接及两个头文件与 Rust 入口同处 `vision/`，构建脚本和原生回归夹具直接引用这些文件。

Windows overlay worker 以实际资源和待办表达生命周期：截图门由 generation、隐藏确认通道和延后帧共同拥有，关闭由 `Shutdown` 待办和 `alive` 控制，不另维护一套阶段枚举。渲染器仍独立记录像素是否已提交，用于决定截图前是否需要原生刷新屏障；这个状态不能由邮箱是否为空推导。

启动预热资源仍归各平台后端所有，正常退出、启动失败后的后端析构及运行时错误沿用各自的 `shutdown_resources`，共享关闭截止时间并收集全部清理错误；构造尚未完成时由已创建资源的 RAII 兜底。不另设预热资源管理器或独立关闭流程。Windows 渲染线程若在 GPU 初始化前已收到关闭请求，直接清空原有邮箱并确认关闭，不再创建即将销毁的设备；已经进入原生初始化的调用仍由原线程完成并沿既有关闭流程回收。

两端通过 `Backend::stop_input_capture` 先关闭同步按键处置邮箱：唤醒正在等待的回调，后续回调不再排队等待已经退出的 Engine。引擎在退出清理入口调用，覆盖配置和工作区保存的等待；后端自身关闭时重复调用，覆盖启动失败和析构。Windows 保留原生按下／松开配对及钩子线程，继续执行已排队的合成释放，到原定阶段才停止并 join；macOS 复用原有 Hook 的非阻塞停止请求，键鼠注入仍独立可用。停止 Hook 本身也关闭邮箱；重新启动使用新邮箱，不恢复旧代次。

移动和滚动共用可暂停的原生显示时钟。Windows 在后端启动时预热一个工作线程及当前显示输出，空闲时以条件变量等待；松键只暂停帧投递，不在 Engine 线程等待 VBlank 或 join，线程、DXGI 输出和原生事件在手势间复用，显示拓扑变化使输出缓存失效，退出时才中断并有界回收。macOS 在覆盖层视图可用时预建暂停的 CADisplayLink，同一视图上的手势只暂停／恢复链接，切换来源视图时才重建，退出时 invalidate。两端共用 `common/frame_clock.rs` 的单调计时与有界累计状态：重复启动保留当前手势，暂停清空待交付时间，恢复建立新代号和时间基准，迟到旧帧不能进入新手势，空闲时间不计入距离；待交付的显示帧合并时间而不丢失运动时长。资源复用不增加空闲帧计时器。

冷启动预热通过顺序安排与其他必要初始化重叠：Windows 先启动显示时钟和独立滚动执行器，再准备 GPU、UIA 与托盘；GPU 线程启动不等待消息队列就绪，早期绘制、截图屏障及关闭请求保留在原有邮箱，工作线程建立队列后先消费邮箱再等待原生消息，队列身份与请求用同一把锁交接，避免丢失唤醒。显示快照仍在托盘显示通知源建立后枚举一次，随后唤醒已启动的时钟准备原生输出，避免提前快照漏掉启动期间的显示变更。macOS 在状态图标建立后启动滚动执行器和显示回调目标，与权限、Hook、屏幕及工作区初始化重叠；CADisplayLink 仍在所属视图存在时创建。不要为预热新增线程优先级、QoS、定时器或轮询；首次输入不等待准备完成的握手，移动、滚动和 GPU 原生工作各自独立。提前启动只能提供更多准备时间，不能保证系统资源紧张时的绝对首帧期限。

屏幕归属共用 `common/display_target.rs` 的纯几何缓存：当前矩形内直接复用索引，跨屏时按已有矩形重新定位，空隙取最近矩形；快照替换或覆盖层重建清除旧索引。Windows 在既有显示枚举中同时记录矩形和 HMONITOR，不为帧调用 MonitorFromPoint，也不在启动手势时再次查询鼠标；物理指针通知、已知合成坐标及原有指针查询更新目标。原生显示通知即使返回相同几何也刷新句柄和代号，刷新失败或为空时清除帧时钟的旧句柄，兼容路径使用 DWM 回退；旧输出完成的迟到帧不进入新快照。macOS 用覆盖层已缓存的矩形和视图选择来源，位置帧只在来源视图改变时重建链接，不重复设置 paused，不逐帧查询 NSScreen 或刷新率。

共用缓存和计时不改变原生策略：Windows 一次性动态探测并缓存合成器 API 导出，按实际能力优先使用合成器时钟，不按 Windows 版本名称分支；仅 API 缺失、资源不可用或调用失败时回退到 DXGI VBlank，再回退 DwmFlush，缺失可选 boost 导出不禁用合成器时钟。macOS 保留 NSView 的 CADisplayLink。原生 API、线程归属与资源生命周期仍由各 OS 后端负责。

macOS 在首次显示枚举之前注册通知，保留枚举期间的变更。显示通知即使得到相同逻辑矩形，也重新应用可见面板的 AppKit 位置并检查 backing scale，随后重新选择显示时钟来源；完整场景和位置快路径都先消费这类通知，不能等模式内容变化才更新原生层。面板保留当前位置，显示刷新不会把标记退回上次完整场景的坐标；几何未变时保留窗口和标签存储。正常位置帧只检查通知标记，不重新枚举显示器。

macOS 的 `native::OwnedCf` 只持有非空的 Create／Copy 引用；AX 查询失败或无结果返回空指针时，不创建临时 owner，也不调用 `CFRelease(NULL)`。鼠标窗口识别先走 AX 取点；失败或无法沿关系找到窗口时，才按 Quartz 前后顺序确定鼠标下最上层窗口，并复用编号枚举的 AXWindows 读取，只查询该进程、匹配唯一的普通且非最小化 AX 窗口。回退忽略本进程覆盖层，菜单／浮动层阻止选择后方窗口，不转向活动应用，也不猜测无法区分的重合身份；使用与编号枚举相同的 AX 超时和有限截止时间。正常取点成功不增加枚举。两条路径都失败时由现有 worker 回传结果，Window 模式保留窗口列表和编号选择，不退出进程。

Windows/macOS 的连续滚动共用 `common/scroll_worker.rs`：待执行邮箱由空变为有帧时立即唤醒独立执行线程；已有待执行帧时只替换，不重复通知。不等待上一帧完成、不使用周期计时器，也不与鼠标移动合并。执行器只保留最新未执行帧；原生调用变慢时替换过期帧，不累加距离或补播历史帧，优先响应当前输入。短按仍走各平台原有离散输入路径，Windows 连续帧不占 Hook 的有界注入队列。原生调用在邮箱锁外执行，失败返回 `InputInjectionFailed`；关闭先取消待执行帧，再按共享截止时间回收线程。

公共 `api::scroll::ScrollSession` 为帧提供可取消的代号；模式松键、换向、改速和退出时使旧帧失效，执行器注入前复核。已经进入原生 API 的一次调用可能完成；不得等待它来响应松键。每帧仅克隆共享身份，不新建会话或线程。

## 状态菜单与 About

macOS 使用 AppKit 的 NSMenu 和非模态标准 About 面板，由系统决定外观，不覆盖系统材质。`event_loop.rs` 在主线程派发 AppKit 事件，注册 common、event-tracking 和 modal-panel 模式的 observer 与可复用 deadline timer；原生菜单的嵌套循环期间仍驱动同一个 Engine。普通输入、异步完成和显示帧唤醒原生循环，期限按引擎定时任务及待确认窗口移动调整，不增加固定高频轮询。`display/display_link.rs` 在相同模式注册显示帧。

空闲时不安排周期性的引擎回合。macOS `workspace.rs` 保留 NSWorkspace 应用切换通知及 NSApplication.effectiveAppearance 的 KVO observer，通知只标记缓存失效并唤醒主循环，下一回合读取状态；析构先注销再释放 observer。显示枚举失败和状态图标尚未挂接时按实际重试期限唤醒，恢复后移除期限。Windows 使用消息等待及 `MWMO_INPUTAVAILABLE`，已查看但未移除的消息也必须唤醒；只有前台或托盘通知源无法建立时才降级为 50 ms 轮询。无截止时间使用 INFINITE，有限期限向上取整到毫秒且不能误映射为 INFINITE。

通用回合的有界批处理、事件路由、定时任务和退出清理只存在于 `app/runtime`，两种驱动都复用它。`event_loop.rs` 只拥有 AppKit/CFRunLoop 适配，并集中原来分散在 workspace/display-link 的原生唤醒、等待和事件派发；`workspace.rs` 只缓存焦点与外观。Windows 托盘线程无需这套嵌套循环适配，不能把 AppKit 注册或原生回调寿命搬到公共层。

每个回合有事件数量上限；native-driven poll 不派发 AppKit 事件、不等待，避免持有 Engine/Backend 借用时进入菜单循环。回调通过 RefCell 拒绝重入，在 FFI 边界收束 Rust panic。退出／错误先取消状态菜单跟踪并退出原生循环，注销 observer/timer 后才统一关闭后端资源；注册对象不得比回调上下文活得更久。模式仍遵循原有焦点、点击和完成规则，不因菜单开闭额外进入 idle。

Windows 使用独立托盘线程上的原生菜单和独立对话框线程上的 TaskDialog，保留系统交互与可访问性，不引入 WinUI/WebView 运行时。Win32 控件不承诺自动获得 WinUI 样式或玻璃材质。

更新检查和 Windows 原生对话框的单实例 guard 仅在成功取得标记后构造，由持有者退出时释放；拒绝重复请求不能创建临时 guard 或清除现有标记。对话框 guard 覆盖整个工作线程，共享更新 guard 覆盖整个检查及下载任务，两端后端另保留自己的在途 update worker。

macOS 更新安装由 `update_installer/` 的后台准备任务和独立助手完成。监听 socket 保持非阻塞，以便检查取消和子进程退出；接受的安装握手连接须显式恢复阻塞模式并设置读写超时，因为 Darwin 会继承监听 socket 的非阻塞状态。助手确认接管后主程序才退出，助手等待原进程结束再交换应用包；新版启动确认失败时恢复旧包。启动确认仍使用有截止时间的非阻塞读取，兼容对端写入后立即关闭，不把 socket 等待移到 AppKit／Engine 线程。

窗口树平铺通过 `ApplyLayout.best_effort` 启用公共逐窗口容错：准备阶段跳过已知不可调整／全屏／失效窗口，提交或确认失败只跳过对应窗口，保留其他窗口的成功结果。`Applied.skipped_windows` 回传本轮失败身份，但不删除当前或历史分区关联，也不写永久应用黑名单；拒绝某个矩形不代表窗口永久不可调整。失败反馈本身不触发自动重试，后续分区编辑重新提交目标；编辑事务中的浮动窗口可用窗口编号再选分区重新加入。容错成功也回传查询／观察到的最小尺寸，供公共树算法约束后续边界调整；未变化或仍最大化的尺寸不作为新的最小尺寸证据。只对失败窗口重新查询约束，成功路径不增加原生查询。同步和异步路径共用该语义，取消仍恢复已尝试的操作，非法布局仍整体拒绝，Quick 的严格失败恢复不变。异步确认保持至多 16 个在途窗口与原有截止时间，完成后释放临时记录。部分成功的跳过信息仅作为界面状态，不输出 ERROR；公共模式仅显示窗口编号（最多三个，超出显示剩余数量，失效编号显示 `#?`），不为提示查询原生窗口。整体失败和恢复失败仍保留错误日志。

CGEventTap 的订阅掩码只包含普通输入事件；系统的 TapDisabledByTimeout／TapDisabledByUserInput 是带外回调类型，其值不能移入 64 位事件掩码。停用、恢复与捕获丢失的处理仍留在 callback／tap worker 内。

## 文本输入

两端输入钩子的反向键码表共用 `common::build_key_map`：初始化只遍历原生静态映射一次，保留首个别名优先、非法名称为空及越界代码忽略的语义，不再为数组的每个槽位重新搜索整张表。表仍由各后端的 `OnceLock` 持有和预热；逐键读取、布局字符翻译及注入规则保持各平台所有权，不增加运行期缓存。

UIHint 搜索由共享 Mode 编辑状态、presentation 绘制文字／选区／光标、runtime 拦截按键。两端只提供键盘布局字符与剪贴板，不创建搜索原生控件、不切换窗口焦点。字符捕获仅在搜索会话开启，退出／失败恢复时关闭。当前不提供 IME 组词接口，中文可通过简拼或粘贴输入。

Windows 字符候选只在布局身份变化时请求重建；同一布局下事件的 scan code 缺失或不匹配，继续走保守的原生字符翻译，不反复枚举整个键盘布局。软件键盘和重映射输入仍可绑定，不能用无效重建阻塞退出或重扫按键的 Hook 确认。

窗口备注仍复用原生文本编辑器；调用 AppKit 的显示、关闭、文字设置或焦点方法前必须释放 RefCell 借用，避免同步回调重入。
macOS 应用重新激活时保留已有 field editor；输入法切换中的临时 first-responder 拒绝不能关闭备注面板。初次打开仍检查焦点是否成功建立。

多窗口连续操作由公共 worker 按目标保留小数移动余量和尺寸约束缓存，并在同一手势中合并交错目标的排队增量；撤销记录合并该手势的全部目标。音频批量请求在窗口 worker 解析全部选中窗口及 tab 成员的进程身份，独立 audio worker 按进程身份去重并逐项执行，单项失败不阻断其余目标。音频原生路由的维护唤醒只由 audio worker 管理，窗口 worker 不保留另一套音频维护接口或等待分支。多选关闭 tab 组请求关闭全部成员，保存确认仍由应用处理。

## 点位取样

macOS 取色 filter 与扫描截图复用同一缓存校验：显示器 ID 相同仍须核对逻辑边界及物理像素尺寸，不能在分辨率或 Retina 配置变化后继续使用旧倍率。截图失败或无图像时按 filter 身份清除缓存，让下一次请求重新获取；迟到失败不能清除另一个 filter。

`api/point_sample.rs` 定义一个像素的异步请求／结果；runtime 按 owner 和版本路由，结束清除路由及原生取样状态。Windows 复用 overlay worker 的最新请求槽和 1×1 GDI 缓冲，在自有窗口 region 留一个三像素孔，再等待一次 DWM 提交，避免读取标签／标记且不隐藏整层；取消恢复 region 并释放像素缓冲。macOS 使用 common/point_sample 的单执行／单待办 worker，ScreenCaptureKit 按显示缓存排除本进程的 filter，使用 filter 的 pointPixelScale 将固定全局点映射到最多 32×32 像素的小区域，按原尺寸捕获并裁出指定的单个像素转为 sRGB；返回尺寸不符时拒绝样本，不将整张截图缩小成一个颜色；原生等待有界，超时后的回调自持有资源并释放迟到图像，屏幕捕获许可直到回调完成才释放。退出时取消、停止并有界 join，原生资源不进入模式。

搜索输入与 Point 显示共用固定文字基线和左对齐布局；`OverlayLabel::scroll_to_cursor` 将完整单行文字交给后端，用已有字形位置保持光标可见，非编辑态显示末尾。`trailing_text_len` 标出右对齐的计数后缀，两段文字共用一次布局，按实际计数宽度裁剪查询；查询裁剪还止于自身文字末尾，避免短输入重复绘制后缀。计数宽度及文字区边界在布局变化时计算并缓存，后缀使用 matched_text_color。Windows 复用 DirectWrite 编辑布局或 GDI advance 缓冲；macOS 复用 CoreText 测量和已有裁剪文字子层，光标／选区跟随同一水平偏移。Windows 输入视口使用不随下伸字符变化的固定偏移，空输入也按对齐方式计算插入位置；macOS 固定面板继续按字体行高居中。

`platform/common/text_segmentation.rs` 维护一个正在执行和一个可替换的分词请求，在有界文本块之间检查取消代数。macOS 使用 Foundation 的本地 ByWords 枚举；Windows 使用 Windows 10 起提供的 `Windows.Data.Text.WordsSegmenter`，中文显式请求 zh-Hans。原生 UTF-16 范围在平台层转换并校验为 UTF-8；词典不可用时返回空范围，由模式保留标点并按 grapheme 回退，不附带第三方分词字典。工作线程随后端统一停止，不阻塞引擎输入循环。
