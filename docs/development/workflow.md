# 开发流程与测试

先运行程序，再按改动选择测试。还没确定改哪里，可先看[扩展指南](/development/extension-guide)。

## 环境

| 用途 | 要求 |
| --- | --- |
| Rust 开发 | 使用 `rust-toolchain.toml` 指定的工具链 |
| 文档站 | Node.js 24，通过 npm 安装 pnpm latest |
| 原生验证 | 在目标系统实测；macOS 最低 14 |

## 编译和运行

在仓库根目录运行开发版本：

```bash
cargo run
cargo run -- --help
cargo run -- --doctor
cargo run -- --check -c ./keysteer.default.toml
cargo run -- --dump-config
```

| 选项 | 用途 |
| --- | --- |
| `--doctor` | 检查键盘、显示器、前台应用和权限 |
| `--check` | 只校验配置 |
| `--dump-config` | 输出生效配置 |

::: tip 校验仓库里的配置要带 ./
`--config` 的裸文件名从应用数据目录查找；带目录的相对路径才从工作目录查找。仓库示例用 `./keysteer.default.toml`，PowerShell 也可用 `.\keysteer.default.toml`；绝对路径原样使用。
:::

提交 Rust 改动前，运行：

```bash
cargo check
cargo test --all-features --lib --tests -- --test-threads=1
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

### 分配与性能测试

先读具体测试的环境要求，分配计数串行运行：

```bash
cargo test tests::performance::steady_normal_frames_do_not_allocate -- --ignored --exact --test-threads=1
cargo bench --manifest-path tools/perf/Cargo.toml --bench core_hot_paths
```

`tools/perf/` 是需在隔离副本中适配的本地工具，不属于发布包。不要为基准增加生产 API 或 feature；测试原则见[安全与性能约束](/development/safety-and-performance)。

## 平台打包

使用脚本生成包含图标、签名和所需资源的完整用户包：

```bash
# Windows
powershell -ExecutionPolicy Bypass -File packaging/windows/package.ps1

# macOS
./packaging/macos/package.sh
```

::: info macOS 权限跟随应用身份
实测请运行打包后的 `KeySteer.app`，再按 [macOS 授权步骤](/guide/macos)授权，避免权限落在终端等其他宿主上。
:::

## 文档站

```bash
npm install --global pnpm@latest --allow-scripts=pnpm
pnpm install
pnpm docs:dev
pnpm docs:test
pnpm docs:check
pnpm docs:build
```

| 命令 | 作用 |
| --- | --- |
| `docs:dev` | 同步默认 TOML、图标并启动预览 |
| `docs:test` | 测试配置模型和模拟器 |
| `docs:check` | 检查 Vue／TypeScript 类型 |
| `docs:build` | 生成静态站，并从 GitHub 校验发布信息 |

::: info 构建需要发布信息
开发预览可在网络不可用时继续；正式构建必须成功获取并校验 GitHub Release。遇到构建失败，先区分网络／发布信息错误和页面错误。
:::

模拟器用于预览和编辑，最终配置合法性以程序 `--check` 和 Rust 测试为准。

## 先判断改哪里

| 需求 | 优先改动 |
| --- | --- |
| 按键、速度、主题、应用例外 | TOML |
| 一个按键做几件事 | 动作数组 |
| 新会话状态或交互流程 | Mode |
| 可复用的带参功能 | 内置插件＋Manifest verb |
| 操作系统能力 | `api::Backend` 与平台实现 |
| 新的配置动作语法 | Binding、Command、执行器、测试和文档 |

详细步骤见[扩展指南](/development/extension-guide)。

## 改动建议

### 改绑定或动作

1. 修改 `src/api/binding.rs` 的解析、规范化和序列化。
2. 测试合法／错误输入，以及数组执行顺序。
3. 更新默认 TOML、[动作参考](/reference/modes-and-actions)和模拟器动作分类。
4. 检查 runtime 命令执行、配置编译和集成测试。

### 改 Mode

1. 确认事件、命令和生命周期语义，状态留在 Mode。
2. 用 Command 请求宿主能力。
3. 测试进入、离开、restart、finish；有效配置重载后应使用新实例并回到 Idle。
4. 检查覆盖层、输入释放和切换后的按键 owner。

### 改平台后端

1. 在 `src/api/backend.rs` 定义两平台共用接口。
2. 实现 Windows、macOS，并检查 unsupported。
3. 耗时扫描放到 worker，不阻塞 Engine。
4. 实机验证 Hook、权限、覆盖层、输入注入和多显示器。

## 测试分层

| 测试 | 检查什么 |
| --- | --- |
| Rust 单元测试 | 绑定解析、配置、模式状态、几何算法、路由 |
| `src/tests/` 内部集成测试 | 发布默认值、catalog、CLI、跨平台行为 |
| `tests/` 源码护栏 | 依赖方向、日志策略、安全预算；不导入内部模块 |
| 文档 Node 测试 | 绑定继承、配置副本、模拟器状态 |
| 目标系统实测 | Hook、权限、UIA／AX／Vision、覆盖层、状态菜单、打包 |

纯 Markdown 改动至少检查链接、锚点和以下命令；涉及图表或排版时再看实际预览：

```bash
git diff --check
pnpm docs:check
pnpm docs:build
```

## 文档维护规则

- 默认行为以源码、测试和 `keysteer.default.toml` 为准。
- 数据流变化更新架构；配置语法变化更新用户参考和 AI 手册。
- 中英文页面、章节、示例、图表和导航一起维护，改标题时保留原锚点。
- 先说明用途和操作；提示用 info／tip／warning，details 只留给少数可选长样例。
