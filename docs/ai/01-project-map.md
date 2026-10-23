# 项目地图

路径均相对仓库根目录。

| 位置 | 职责 |
| --- | --- |
| `src/main.rs`、`src/app/mod.rs`、`src/app/cli.rs`、`src/app/bootstrap.rs` | 总入口、CLI、装配与启动 |
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
| `tools/perf/`、`tools/` | 性能基准与原生验收工具 |
| `docs/.vitepress/` | 文档站、配置工作室与浏览器模拟器 |
| `build.rs`、`packaging/`、`.github/workflows/` | 原生构建、打包与发布 |

具体数据流见 [运行时](02-runtime-and-api.md)，依赖方向见 [架构边界](10-architecture-boundaries.md)。

已有子模块的入口与实现放在同一目录，以 `mod.rs` 为入口，例如 `app/preset_store/`、`modes/window/`、`presentation/key_help/`、`platform/common/window_session/`。无子模块的独立职责继续使用单个 `.rs` 文件。`plugins/` 直接保存内置插件及其装配，不再增加中间命名空间。

紧密相关的小实现共用职责文件：普通／递归网格在 `presentation/grid.rs`，窗口历史及撤销确认在 `platform/common/window_session/history.rs`，Windows 系统通知在 `platform/windows/system_events.rs`，GDI 截图与像素资源在 `platform/windows/native/gdi.rs`。绑定解析测试随 `api/binding.rs` 保存；独立的取消、线程和资源生命周期仍保留自身边界。

Windows/macOS 的 `input/` 集中输入捕获、映射和注入，`display/` 集中显示枚举、覆盖层和帧时钟，`vision/` 保存视觉识别实现及所属桥接。平台装配仍在各 OS 的 `mod.rs`；原生资源与线程所有权见 [平台后端](06-platform-backends.md)。

现有 `app::run_cli` 入口经 `platform::run` 进入所选平台的进程入口，再调用 CLI；平台私有工作模式的参数解析及执行留在对应 OS 目录。
