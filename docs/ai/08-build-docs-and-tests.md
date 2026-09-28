# 构建与验证

工具链、依赖和脚本以 `rust-toolchain.toml`、`Cargo.toml`、`package.json` 为准。

## Rust

按改动先跑相关测试；运行时或跨层改动使用：

```sh
cargo fmt --check
cargo test --all-features --lib --tests -- --test-threads=1
cargo clippy --all-targets --all-features -- -D warnings
```

架构、日志和 unsafe 护栏分别在 `tests/architecture_dependencies.rs`、`tests/logging_policy.rs`、`tests/safety_budget.rs`。不要为通过检查而直接放宽预算。

正式构建使用 `cargo build --locked --release --bin keysteer --no-default-features`。单元测试由 `cfg(test)` 隔离，测试依赖放在 dev-dependencies；基准与原生探针位于独立包 `tools/perf/`，不作为主包构建目标。`benchmark-hooks` 默认关闭，仅性能包显式启用并加载 `tools/perf/support/benchmark.rs`。

## 性能与原生验证

- `src/tests/performance.rs` 和模块内 ignored 测试覆盖分配与原生探针；先读测试的环境要求，分配计数串行运行。
- `cargo bench --manifest-path tools/perf/Cargo.toml --bench core_hot_paths`：核心 CPU 路径；`--bench notification_queue`：通知队列。
- `tools/` 提供 A/B、整进程和原生测量入口。正式性能比较不启用 `perf-probe`，基线与候选用独立 target 目录。
- 原生变更检查对应 target，并在对应 OS 验证输入、权限、窗口、显示与清理。交叉编译只证明编译兼容。
- 失败先区分本轮回退和已有基线问题；忽略测试不等于验收通过。

## 网页与发布

网页代码位于 `docs/.vitepress/`；变更时按范围运行 `pnpm docs:test`、`pnpm docs:check`、`pnpm docs:build`，视觉交互另做浏览器验证。

正式产物走 `packaging/` 脚本和 `.github/workflows/`，检查应用身份、资源和签名。纯文档修改验证链接、代码路径和 `git diff --check` 即可。

`.github/workflows/cross-build.yml` 是手动触发的 Linux → Windows MSVC x64/ARM64 构建，使用 cargo-xwin/LLVM，不发布 Release。`packaging/windows/package-cross.sh` 对已编译 EXE 使用现有 `WINDOWS_SIGNING_PFX_BASE64`、`WINDOWS_SIGNING_PASSWORD` 和可选 `WINDOWS_TIMESTAMP_URL` 完成 Authenticode 签名、RFC 3161 时间戳和签名校验；缺少证书或签名失败时不上传包。产物名称与 `package.ps1` 一致：`KeySteer-v<version>-<target>.zip` 内含 `KeySteer/KeySteer.exe` 和 `KeySteer/keysteer.default.toml`。签名校验显式信任提供的证书链并验证叶证书指纹，不代表公共信任或 Windows 原生运行验证。正式发布仍使用 `build.yml`。

两个构建工作流复用 `.github/actions/setup-rust`，统一从 `rust-lang/rust` 官方 Release API 解析最新稳定版本，由 `dtolnay/rust-toolchain` 安装工具链与目标架构并设置构建日期。仅在 Cargo.toml 与 Cargo.lock 的项目版本不同步时调用 `cargo update`，避免每次初始化都更新索引。ZIP 上传关闭二次压缩；原生 Windows 签名证书在打包结束或失败后清理。

交叉工作流通过带 `GITHUB_TOKEN` 的官方 Release API 查询 LLVM 和 cargo-xwin：`llvm/llvm-project`、`rust-cross/cargo-xwin`。LLVM 下载 Linux X64 工具包（优先 zstd），按 Release API 的 SHA256 校验，支持 1 GiB zstd 解压窗口，解压后将 bin 目录置于 PATH 首位；不访问 `apt.llvm.org`。cargo-xwin 由 `taiki-e/install-action` 下载并校验上游预编译程序，禁用 fallback，避免从源码安装。系统库和签名工具使用 Ubuntu 软件源。

缓存按存储成本收敛：`build.yml` 只缓存 Rust 依赖；`cross-build.yml` 保留 Rust 编译结果、Windows SDK/CRT、LLVM 压缩包、cargo-xwin 可执行文件和 apt 安装包。LLVM 与 cargo-xwin 缓存按宿主系统、架构和工具版本或摘要区分，不按产物目标区分，因此两个 Windows 目标共用；未来同一 Linux 宿主工具链也可供 macOS 交叉构建复用，原生 macOS runner 则需要对应宿主工具包。LLVM 缓存恢复后仍校验 SHA256 并解压。

仅取消逐提交未签名 EXE 缓存。apt 安装包缓存按宿主和软件源元数据区分，并在目标架构之间共用。交叉构建由 Cargo 判断需要重编译的内容，不再通过 EXE 缓存完全跳过编译步骤；Rust 缓存按目标、CPU 与编译工具版本区分，不受签名工具的 apt 元数据影响。两者的 Rust 缓存允许失败后保存可用编译结果，原生构建仍只在默认分支写入缓存。SDK/CRT 强制刷新时提升 `xwin-sdk-v1` 前缀。旧缓存不会因工作流配置修改自动删除，需要在 GitHub Actions 缓存管理中清理或等待淘汰。证书和密码不进入缓存或产物；签名后 EXE 和发布 ZIP 的 SHA256 只在 Actions 摘要记录。

交叉工作流的 x64 产物按实际 GitHub Actions runner CPU 优化，Rust 设置 `target-cpu=native`，C 桥接设置 `/clang:-march=native`。CPU 型号和指令集指纹写入编译缓存键，避免不同 runner 硬件复用不兼容的机器码；工具和 SDK 下载缓存仍可复用。这不是通用 x86-64 兼容包，缺少构建机器指令集的 CPU 可能无法运行，runner 硬件变化也可能改变产物。ARM64 在 x64 runner 上交叉编译，保持 `generic`。两端继续使用 Cargo release 的 O3、fat LTO 和单代码生成单元，最新工具链与 CPU 定向优化不等于已证明更快，性能仍需同机实测。

`KEYSTEER_CROSS_WINDOWS=1` 在非 Windows 宿主上启用 C 桥接和图标/manifest 编译，需要完整交叉工具链；未设置时保留不依赖 SDK 的跨平台类型检查。MSVC 交叉工作流显式设置 `RC_PATH=llvm-rc` 和空 `CROSS_COMPILE`，避免 winresource 0.1.31 初始化 GNU 前缀时误报 ARM64 未知目标。跨宿主的工具链、SDK、路径和签名时间戳可能改变哈希，不能据此断言行为不一致。
