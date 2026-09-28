# 项目地图

路径均相对仓库根目录。

| 位置 | 职责 |
| --- | --- |
| `src/main.rs`、`src/app/cli.rs`、`src/app/bootstrap.rs` | CLI、装配与启动 |
| `src/api/` | 跨层命令、事件、数据与能力端口 |
| `src/config/` | TOML 模型、默认值、解析、校验与写回 |
| `src/app/configuration.rs`、`src/app/mode_catalog.rs` | 编译配置、注册模式 |
| `src/app/runtime/` | Engine、输入路由、调度、命令执行 |
| `src/modes/`、`src/plugins/` | 平台无关状态机 |
| `src/presentation/` | 借用模式视图，生成场景与布局 |
| `src/platform/common/` | 共享扫描、窗口协调和消息设施 |
| `src/platform/windows/`、`src/platform/macos/` | 原生实现；由 `src/platform/mod.rs` 选择 |
| `src/support/` | 统一日志、错误汇总、worker 生命周期 |
| `src/app/paths.rs`、`src/app/preset_store/` | 应用数据路径、工作区持久化 |
| `tests/`、`src/tests/`、模块内测试 | 架构护栏、集成与单元测试 |
| `benches/`、`tools/` | 性能基准与原生验收工具 |
| `docs/.vitepress/` | 文档站、配置工作室与浏览器模拟器 |
| `build.rs`、`packaging/`、`.github/workflows/` | 原生构建、打包与发布 |

具体数据流见 [运行时](02-runtime-and-api.md)，依赖方向见 [架构边界](10-architecture-boundaries.md)。
