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

窗口树平铺通过 `ApplyLayout.best_effort` 启用公共逐窗口容错：准备阶段跳过已知不可调整／全屏／失效窗口，提交或确认失败只跳过对应窗口，保留其他窗口的成功结果。`Applied.skipped_windows` 回传本轮失败身份，模式清除其当前及历史分区分配，防止后续编辑自动重试；不写永久应用黑名单。同步和异步路径共用该语义，取消仍恢复已尝试的操作，非法布局仍整体拒绝，Quick 的严格失败恢复不变。异步确认保持至多 16 个在途窗口与原有截止时间，完成后释放临时记录。

## 文本输入

UIHint 搜索由共享 Mode 编辑状态、presentation 绘制文字／选区／光标、runtime 拦截按键。两端只提供键盘布局字符与剪贴板，不创建搜索原生控件、不切换窗口焦点。字符捕获仅在搜索会话开启，退出／失败恢复时关闭。当前不提供 IME 组词接口，中文可通过简拼或粘贴输入。

窗口备注仍复用原生文本编辑器；调用 AppKit 的显示、关闭、文字设置或焦点方法前必须释放 RefCell 借用，避免同步回调重入。
macOS 应用重新激活时保留已有 field editor；输入法切换中的临时 first-responder 拒绝不能关闭备注面板。初次打开仍检查焦点是否成功建立。
