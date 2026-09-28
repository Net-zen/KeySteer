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
