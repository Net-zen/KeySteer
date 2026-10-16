# 安全与性能约束

改输入、绘制或异步任务时，先确保三件事：按键立即响应，退出后没有卡住的输入，旧任务不影响新会话。

## 关键不变量

| 改动位置 | 必须保留的行为 |
| --- | --- |
| 输入 Hook | 立即返回 Consume／Forward；不做扫描、配置 I/O、进程等待或 sleep。Defer 仅兼容保留，内置运行时按 Forward 处理 |
| 按键归属 | 同一物理键的按下和松开交给同一个 owner；切换模式后，原 owner 仍负责结束持续动作 |
| Mode／插件 | 只用 ModeEvent、Command、HostContext；原生调用留在 Backend |
| Rust 边界 | api、app、config、modes、plugins、presentation 使用 safe Rust；FFI 集中在平台层 |
| 命令存储 | 常见 0／1／2 条命令用安全的 CommandBatch 内联存储，不引入裸指针、MaybeUninit 或手写 drop |
| 会话清理 | 鼠标按住、点击提示、动作序列、扫描、覆盖层和帧时钟都有结束、取消、重载、禁用与退出路径 |

::: info 无效重载不改变当前会话
配置校验失败时保留有效配置和运行状态；有效重载才重启运行时。两种情况都要保持物理按下／松开的配对关系。
:::

## 连续移动和显示帧

位移按每帧实际经过的 `elapsed` 计算，不能按帧数或固定 16 毫秒估算。60 Hz、144 Hz 和偶发掉帧下，同样时长应得到一致的总位移。

| 平台 | 显示帧来源 |
| --- | --- |
| Windows 11 | DirectComposition compositor clock，同时等待 stop event |
| 旧 Windows | 当前输出的 DXGI VBlank，再回退 DwmFlush |
| macOS | 绑定覆盖层 View 的 CADisplayLink，随所在显示器切换 |

等待显示帧放在 worker，不阻塞 Hook、Engine 或覆盖层线程，也不轮询刷新率。停止时唤醒、join 并释放 worker。

平滑加速用 smootherstep，关闭后用线性加速；释放最后一个方向键立即停止。长帧跨过加速终点时，分别计算加速段和匀速段；亚像素余数、对角线归一化和速度倍率也按实际帧间隔处理。

## 覆盖层与异步任务

| 路径 | 保留的做法 |
| --- | --- |
| Windows 提交画面 | 最新帧单槽＋共享 OverlayScene；提交不等绘制完成 |
| 静态标签与形状 | 写时复制；鼠标或提示变化时不复制、排序或比较整张 Grid |
| Windows 高 DPI 缓存 | 仅静态标签或缩放变化时重建；空场景、100% 缩放、Hide／Shutdown 时释放 |
| macOS 覆盖层 | UI 回正确线程；Hide 断开 content view／layer tree，不留大型场景占位 |
| UIA／AX／Vision 扫描 | worker 分批返回，带 scan id 和 owner；丢弃旧模式、旧屏幕或取消后的结果 |
| 扫描内存 | 活动期间复用数组，退出大型扫描后释放大块分配 |

新增异步任务前，明确四项：**谁拥有结果、如何取消、队列／结果上限、怎样交付给 Engine**。交付不能阻塞事件循环。

## 变更检查清单

在目标系统实测，并运行下方检查：

- 按下、松开、重复、切换模式和退出后，没有按键或鼠标卡住。
- 低／高刷新率、跨屏和掉帧时，移动距离仍按实际时间计算。
- 失败恢复不残留覆盖层、点击颜色、定时器、扫描或帧 worker。
- Windows、macOS 有对应行为，unsupported 返回清晰错误。

### Windows 输入失败

| 检查项 | 要求 |
| --- | --- |
| 失败证据 | 保留 SendInput 实际插入数量和即时 GetLastError；零返回不能一律判定为权限问题 |
| 诊断成本 | session、integrity、elevation、UIAccess 等查询和日志格式化只在失败分支执行；成功路径不增加诊断分配或系统查询 |
| 点击降级 | 优先完整批次；仅插入数为零时逐边沿重试，部分成功不能重放 |
| 释放兜底 | 降级失败时保守 Release，退出时仍释放鼠标按钮 |

### 避免 Hook 与 Engine 互相等待

Engine 决定按键去向后，不能再同步等待低级 Hook 执行 SendInput；下一次输入可能已让 Hook 等待 Engine，形成死锁。

鼠标按钮、滚轮、键盘注入使用预分配的 **32 项有界 FIFO**，Hook 返回消息循环后串行执行，相邻请求合并唤醒；失败再异步复位输入。保持队列有界，不改为轮询、sleep、额外注入线程或移动热路径上的等待。

发布前运行：

```bash
cargo fmt --check
cargo test --all-features --lib --tests -- --test-threads=1
cargo clippy --all-targets --all-features -- -D warnings
cargo doc --no-deps
pnpm docs:check
pnpm docs:build
```

性能测试使用隔离的本地适配，不扩大生产 API。更多运行方式见[开发流程与测试](/development/workflow)。
