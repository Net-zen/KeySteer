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
