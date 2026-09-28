# 模式与生命周期

Mode/Plugin 是平台无关状态机：接收 `ModeEvent` 和 `HostContext`，返回 `CommandBatch`，经 Presenter 提交视图。不能直接调用原生 API、读配置或依赖具体 renderer。

## 代码入口

| 能力 | 位置 |
| --- | --- |
| 空闲、连续移动 | `src/modes/idle.rs`、`src/modes/normal.rs` |
| 网格与共享定位状态 | `src/modes/grid.rs`、`src/modes/recursive_grid.rs`、`src/modes/targeting.rs` |
| Hint 标签、搜索、扫描会话 | `src/modes/hint/` |
| 窗口模式、编辑、分组与恢复 | `src/modes/window.rs`、`src/modes/window/` |
| 临时文本透传 | `src/modes/text_input.rs` |
| 插件 | `src/plugins/builtin/` |
| 注册与生命周期词汇 | `src/app/mode_catalog.rs`、`src/api/lifecycle.rs` |

## 关键约束

- targeting `keep` 保留同一实例及选择状态；只有 `restart` 发 `Restarted`。
- Finish 应幂等，点击后的生命周期动作不得递归触发点击。
- modal Pop 恢复下层状态，不把恢复当成重新激活。
- Idle 不捕获普通输入；其他模式的输入兴趣通过 API 声明，Engine 不硬编码模式内部键表。
- 会话结束取消其异步工作并隔离迟到结果；持久窗口分组与当前模式会话的寿命分开。
- Hint 已输入前缀时，迟到扫描结果不能重新分配正在使用的键码。
- Hint 搜索可在首批结果到达前打开；空结果重试保留输入与搜索框，后续结果继续按当前查询过滤。

搜索编辑使用 `api/text_edit.rs` 的 Unicode 光标／选区操作，复用会话查询缓冲。修改文字时过滤，移动光标只重绘。复制信息槽成功后退出搜索，复制输入选区保留编辑。
