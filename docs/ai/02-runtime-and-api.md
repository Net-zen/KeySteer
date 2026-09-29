# 运行时与公共 API

```text
TOML → ConfigFile → app::configuration → RuntimePlan → Engine
                                                     ↕ ModeEvent / Command
                                                  Mode / Plugin
Engine → Backend → OS
Mode → HostContext::present(View) → presentation → OverlayScene
```

## 所有权与入口

`src/app/runtime/mod.rs` 装配 Engine；`registry.rs` 管模式和路由，`input_state.rs` 管输入配对，`scheduler.rs` 管待执行任务，`overlay_coordinator.rs` 管呈现，`commands.rs` 解释命令。这些文件位于 `src/app/runtime/`。

跨层词汇在 `src/api/backend.rs`、`src/api/command.rs`；原生对象不进入模式。

## 关键语义

- 同步 Hook 先取得 consume/forward 决定，再执行可能耗时的动作；不得让 Hook 与 Engine 互相等待。
- Down/Up 保持配对；held release 交给按下时的 owner，不在松键时重新解释绑定。
- 退出、暂停、捕获丢失及失败恢复都要清理合成保持状态；先尽力完成恢复，再统一记录错误。
- 成功的合成 click/double-click 只发一次语义 `Clicked`；物理点击和 press/release/toggle 不发。
- 异步结果按请求、会话和 owner 归属处理；迟到结果不能恢复已经退出的交互。
- 输入优先，但后台通知不能无限推迟就绪帧；可靠完成事件保序，只有明确可替换的状态通知允许合并。

模式切换见 [生命周期](04-modes-and-lifecycle.md)，配置替换见 [配置](03-configuration.md)。

UI 扫描的 UiScanActivationExpected 按 scan owner 路由，必须在对应原生焦点通知前送到模式，防止一次主动激活触发重复扫描。范围选择与跨平台激活策略见 [UI 扫描](05-ui-scanning.md)。

UIHint live TextPrompt 仅建立共享输入路由，不调用原生 request_text_prompt。runtime 在任何编辑、剪贴板和绘制工作前确认按键处置；TextInserted／TextEdit／TextPasted 交给 Mode，原生备注仍走原有异步协议。

输入会话完成（含取消、错误）须先关闭字符捕获，再移除 owner 并派发结果；关闭命令重复执行也要确保捕获关闭。旧请求的迟到结果不得关闭新会话，live 搜索结束不进入原生窗口的焦点恢复流程。

原生布局保存备注期间冻结模式的焦点快照，输入法辅助进程、候选窗口及应用重新激活不能取消编辑；会话由明确的提交／取消、模式退出或暂停收尾。普通按键和输入法切换快捷键继续转发给原生编辑器。
