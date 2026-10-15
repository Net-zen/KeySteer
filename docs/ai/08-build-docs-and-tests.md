# 构建与验证

工具链、依赖和脚本以 `rust-toolchain.toml`、`Cargo.toml`、`package.json` 为准。

原生 macOS 打包使用 macOS 26 runner 的 AppKit SDK，使标准菜单和 About 能采用新系统外观；最低部署版本仍由 `packaging/macos/package.sh` 定义为 14.0。旧 SDK 编译的本地包不保证启用新系统设计；外观仍遵循用户辅助功能设置。

## Rust

按改动先跑相关测试；运行时或跨层改动使用：

```sh
cargo fmt --check
cargo test --all-features --lib --tests -- --test-threads=1
cargo clippy --all-targets --all-features -- -D warnings
```

架构、日志和 unsafe 护栏分别在 `tests/architecture_dependencies.rs`、`tests/logging_policy.rs`、`tests/safety_budget.rs`。不要为通过检查而直接放宽预算。`safety_budget` 另编译实际 macOS CF owner 源码，用检查空指针的模拟 release 验证空 AX 输出、单次释放与所有权转移；可在非 macOS 宿主运行，但不代替原生实机验证。

正式构建使用 `cargo build --locked --release --bin keysteer --no-default-features`。单元测试由 `cfg(test)` 隔离，测试依赖放在 dev-dependencies。主包不暴露基准接口或对应 feature，`--all-features` 不依赖本地性能目录。性能测试、适配代码、独立锁文件及结果只保留本地，不纳入提交；需要访问内部实现时在隔离副本内适配，不能为基准扩大生产 API。

## 性能与原生验证

以下 `tools/` 命令是本地工具的使用示例，需先在隔离副本中准备相应适配；主包不提供供这些工具调用的公开测试接口。A/B 两边使用同一工作量、工具链、锁定依赖和构建参数。

- `src/tests/performance.rs` 和模块内 ignored 测试覆盖分配与原生探针；先读测试的环境要求，分配计数串行运行。
- `cargo bench --manifest-path tools/perf/Cargo.toml --bench core_hot_paths`：核心 CPU 路径；`--bench notification_queue`：通知队列。
- `cargo bench --manifest-path tools/perf/Cargo.toml --bench scan_stream -- --fusion-only` 比较就绪数据的融合 CPU 成本；`-- --async` 使用实际融合 worker 比较首份额、提交耗时、大来源期间的小来源延迟与总耗时。`-- --async-hint --common` 补测 200／300／400 标签的实际异步融合与共享 Hint 场景生成，分别记录首次场景、全量完成及后到小来源响应。基准不包含原生识别或系统合成器，不能冒充输入到像素响应。
- `tools/uihint-replay/` 是独立的流式识别回放包：`cargo bench --manifest-path tools/uihint-replay/Cargo.toml --bench scan_replay -- --samples 32 --output replay.json` 默认测试后到 OCR 的元数据更新，并在每次消费后派发一次生产预热计时器，最后排空剩余预热；`--no-maintenance` 可单独诊断就绪数据处理；`--first-scene-only` 固定首份额 16 项、即时消费邮箱，并在首条 `ShowOverlay` 时停止计时，用于快速单独核对首批响应（正确性仍回放完整扫描）；A/B 脚本对应开关为 `-FirstSceneOnly`；`--fixtures <目录>` 可加入包含 bounds／contours／ocr／ax／visible 的本地冻结结果，`--verify` 增加逐项交付，`--features allocations` 单独测分配。程序遍历来源顺序、批次及邮箱合并，核对参考交付顺序的完整最终目标，并记录仍存在区域的标签变化、预热前首份场景、含后续维护的总耗时及最慢份额。私有界面数据留在本地，不写入仓库夹具。异步 worker／邮箱／Hint／最终场景的确定性组合回归在 `src/modes/hint/tests/scan_replay.rs`，另覆盖纯视觉、缩放及旧请求隔离。
- 回放 A/B 使用相同依赖与工作量、独立 target 目录，编译和检查全部结束后交替运行至少三轮，保存为 baseline-N.json／candidate-N.json；`tools/benchmark-uihint-ab.ps1` 以相同亲和性和 Normal 优先级交替运行并记录环境。`python tools/compare-uihint-replays.py <目录>` 先核对原始输入的逐位坐标／内容指纹，核对最终结果与标签变化，再比较各场景各分位数的轮次中位数；默认同时超过 3% 和 2 微秒视为需调查的 CPU 变化，分配不使用时间容差。明确增加目标的修复可用 `--changed-fixture` 标记预期的识别／编号差异，性能与分配门槛不变；其余界面仍严格核对完整结果。保留失败记录并排查，不用调整门槛掩盖回退。这些指标只包括已就绪数据的融合和共享展示，不包含截图、OCR／UIA／AX 遍历或系统上屏。
- `tools/` 提供 A/B、整进程和原生测量入口。正式性能比较不启用 `perf-probe`，基线与候选用独立 target 目录。

`tools/benchmark-core-ab.ps1 -Mode Gate` 交替运行 Hint 扩展场景、持续移动帧、小规模标签和引擎按键派发，保存实际 EXE 的 SHA256 与每轮原始输出。`compare-core-benchmarks.py` 汇总各轮分位数的中位数，默认 p50 或 p95 同时超过 3% 和 2 ns 就返回非零；p99 保留为尾延迟诊断。阈值用于发现需要调查的变化，不代表阈值以内没有退化。正式比较至少三轮，异常时增加轮次并保留原始失败记录；计时期间不运行编译、测试或其他基准。CPU 门禁不能替代原生输入到像素验证。

基准子进程默认使用 `Normal` 优先级；`-BenchmarkPriority AboveNormal` 可用于排查调度干扰，须单独保存结果，不能替代默认优先级的响应测试。环境记录包含优先级和计时器分辨率；比较器同时检查成对轮次与样本批次，避免工作量不同的结果混比。
- 原生变更检查对应 target，并在对应 OS 验证输入、权限、窗口、显示与清理。交叉编译只证明编译兼容。`native_macos_window_parity` 还直接检查鼠标窗口枚举回退，并确认恢复的 AX 窗口与编号枚举保留同一身份。
- macOS 取色回归夹具：`clang -std=c11 -Wall -Wextra -Werror tests/fixtures/macos-point-capture.c -framework CoreGraphics -o /tmp/keysteer-point-capture && /tmp/keysteer-point-capture`；验证 Retina／副屏坐标、固定像素不混入变化的邻域颜色，以及错误尺寸拒绝。非 macOS 可省略 framework，仅运行坐标测试。
- 失败先区分本轮回退和已有基线问题；忽略测试不等于验收通过。

## 网页与发布

网页使用 Node.js 24，先用 `npm install --global pnpm@latest --allow-scripts=pnpm` 安装或升级 pnpm，再运行 `pnpm install`；npm 12 需要允许 pnpm 自身的安装脚本以放置原生可执行文件。本地通过 npm 管理 pnpm，不通过 `packageManager` 固定版本；Pages 每次从 npm 的 `latest` 标签解析最新正式版，并以解析出的确切版本缓存工具。`pnpm-workspace.yaml` 通过 `runtimeOnFail: ignore` 将语言工具链留给既有工具管理，避免仅安装文档依赖时下载 Rust。VitePress 2 当前固定使用 alpha 版本，采用 Vite 8 的 Rolldown／Oxc 构建链，不安装 esbuild；自定义 Markdown 测试须等待 `renderAsync` 完成异步语法高亮。

网页代码位于 `docs/.vitepress/`；变更时按范围运行 `pnpm docs:test`、`pnpm docs:check`、`pnpm docs:build`，视觉交互另做浏览器验证。

`docs:dev`、`docs:check`、`docs:build` 通过 `scripts/docs.mjs` 在同一个 Node 进程中先同步资源，再启动对应 CLI，避免嵌套启动 pnpm 和额外的 Node／命令包装。CLI 选项原样透传；正式构建仍从 GitHub 获取并校验所选 Release。`latest-release.ts` 兼容 `dev`／`serve` 字符串及 VitePress 开发 CLI 转交的选项对象，网络失败可继续预览；`build` 和未知命令始终严格校验。

正式产物走 `packaging/` 脚本和 `.github/workflows/`，检查应用身份、资源和签名。纯文档修改验证链接、代码路径和 `git diff --check` 即可。

仓库根目录的 `Casks/keysteer.rb` 和 `bucket/keysteer.json` 分别作为自定义 Homebrew tap 与 Scoop bucket，安装时显式提供当前仓库 Git URL。`packaging/package-managers/generate.py` 从正式 Release 元数据生成四架构的下载 URL 与 SHA256；提供本地附件时重新计算并核对摘要，缺包、非法来源、草稿和预发布均拒绝写入。Scoop x64 固定选择无后缀的默认 AVX2 包，兼容版／AVX-512 附件不会覆盖默认选包。Scoop 升级／卸载前用 hooks 备份用户 TOML 和 `workspace.ksw`，安装前还原；避免原子保存替换文件后破坏持久化硬链接，默认配置和程序每次使用新包。macOS 沿用 Application Support 目录。

`build.yml`（Build）是唯一编译与发布入口；`pages.yml`（Pages）保留文档部署入口。Build 的 `build_mode` 默认 `cross`，可显式选 `native`；`platform` 支持 windows／macos／all。tag 推送自动用 cross 发布；手动支持平台选择、发布开关（默认开启）和 release/pre-release 类型。发布开启时始终编译两平台全部基础包，cross 另含兼容／AVX-512 两包；只构建时才按平台筛选。Cross 在 Ubuntu 编译，Native 使用 Windows 2025 与 macOS 26；未选模式的任务跳过，不启动 runner。

Build 的 Prepare 任务同时负责入口转发与源码校验：tag 推送和非默认分支手动运行只转发一次到默认分支的同一 `build.yml`，传递源码 SHA、tag、入口 run id 和入口 commit；默认分支运行直接构建，不再次转发。删除 tag 不运行。实际编译保持默认分支缓存作用域，checkout 固定为所选提交；显式 tag 必须指向该提交且已包含在默认分支历史中。完整 cross tag 发布共 10 个 runner job：入口、源码校验、四个 Windows／两个 macOS 构建、发布与清单同步、Pages。清单同步复用发布 runner，入口清理复用 Pages runner。

`.github/scripts/release.py` 校验附件并调用 `tools/compose-release-notes.sh`，从 `docs/releases/index.md` 提取当前版本，再追加 `.github/release-notes.md` 的通用安装／兼容说明。它校验远端 tag 的最终 commit；存在时保留 tag 对象及注释／签名，不一致则报错，缺失时才创建。cross 可重建同名 Release 和附件，但不删除 tag；native 拒绝重复 Release。预发布不占用 Latest。默认分支发布共用 concurrency 组，新运行取消旧运行，入口使用独立组以免与实际构建互相取消。

发布任务在 Release 成功后继续读取 latest 正式版、下载附件、验摘要并将 Homebrew／Scoop 清单提交到默认分支；预发布不会成为清单版本。清单生成器及测试从默认分支单独检出，构建旧 tag 时也使用当前发布工具。需要 `GITHUB_TOKEN` 的 `contents: write` 且默认分支允许 bot 提交，受保护分支需按仓库规则处理。包管理测试入口为 `python -m unittest discover -s packaging/package-managers -p 'test_*.py'`；PowerShell 可用时执行真实的配置升级 hooks。

Pages 通过 `workflow_call` 在 Build 内执行，同时保留独立手动入口；调用者 ref 保持默认分支以满足 github-pages 环境规则。仅在所选平台构建和发布／清单同步成功（或未选发布而跳过）后部署；发布使用本次 release tag，仅构建使用 `latest`。部署实际成功后复用同一 runner：`.github/scripts/cleanup_caches.py` 读取仓库全部缓存分页，按最后访问时间删除闲置超过 7 天的缓存，仓库变量 `ACTIONS_CACHE_MAX_IDLE_DAYS` 可改为其他正整数天数；按缓存 ID 删除，不按 key 批量删除，近期使用的旧缓存也保留。缓存清理失败只记录警告，不影响部署和后续入口清理。最后调用 `.github/scripts/cleanup_entry.py` 检查所选模式的全部阶段结果，并删除对应且成功完成的入口运行记录。由于入口与实际构建共用 `build.yml`，入口清理还查询 jobs，确认只有 Prepare 成功、其余任务全部跳过；含实际构建／部署工作的运行不会被删除。失败／取消、来源不匹配或无入口 id 时保留入口记录。版本 tag、实际 Build 记录及无关运行始终保留。Prepare 和清理步骤需要 `actions: write`；Pages 使用 `contents: read`、`pages: write`、`id-token: write`。发布与清理测试入口是 `python -B -m unittest discover -s .github/scripts -p 'test_*.py'`。

`packaging/windows/package-cross.sh` 对已编译 EXE 使用现有 `WINDOWS_SIGNING_PFX_BASE64`、`WINDOWS_SIGNING_PASSWORD` 和可选 `WINDOWS_TIMESTAMP_URL` 完成 Authenticode 签名、RFC 3161 时间戳和签名校验；缺少证书或签名失败时不上传包。默认 AVX2 版产物名称与 `package.ps1` 一致：`KeySteer-v<version>-<target>.zip` 内含 `KeySteer/KeySteer.exe` 和 `KeySteer/keysteer.default.toml`；x64 兼容版／AVX-512 版在 target 后追加 `-compatible`／`-avx512`，上传 artifact 身份也独立，发布时校验六个精确文件名。更新器复用同一套完整 v3／v4 特性要求：编译期特性识别当前构建，运行时 CPU 和系统可用特性选择支持的最高等级（AVX-512 → AVX2 → 兼容），不受最初下载包影响。新版本正常升级；同一版本但构建等级不同也下载切换，已经匹配时不重复下载，不降级版本号。GitHub Release API 直连失败后，只通过 `gh-proxy.com/https://api.github.com/repos/dccif/KeySteer/releases/latest` 重试，不使用 jsDelivr。两路共用包身份、大小和摘要校验；镜像检查成功时下载优先走相同镜像，失败再试直连。旧版更新器仍请求无后缀文件，因此不支持 v3 的旧电脑首次迁移需手动下载兼容包。签名校验显式信任提供的证书链并验证叶证书指纹，不代表公共信任或 Windows 原生运行验证。原生构建通过 `build_mode=native` 选择；两种模式使用相同的产物名称和发布说明来源，原生模式仍拒绝重复 Release。

两种构建模式复用 `.github/actions/setup-rust`，统一从 `rust-lang/rust` 官方 Release API 解析最新稳定版本，由 `dtolnay/rust-toolchain` 安装工具链与目标架构并设置构建日期。仅在 Cargo.toml 与 Cargo.lock 的项目版本不同步时调用 `cargo update`，避免每次初始化都更新索引。ZIP 上传关闭二次压缩；原生 Windows 签名证书在打包结束或失败后清理。

交叉工作流通过带 `GITHUB_TOKEN` 的官方 Release API 查询 LLVM 和 cargo-xwin：`llvm/llvm-project`、`rust-cross/cargo-xwin`。LLVM 下载 Linux X64 工具包（优先 zstd），按 Release API 的 SHA256 校验，支持 1 GiB zstd 解压窗口，解压后将 bin 目录置于 PATH 首位；不访问 `apt.llvm.org`。cargo-xwin 由 `taiki-e/install-action` 下载并校验上游预编译程序，禁用 fallback，避免从源码安装。系统库和签名工具使用 Ubuntu 软件源。

两种构建模式通过共享 Rust 初始化缓存工具链及第三方依赖下载，不缓存工作区源码、`target` 或逐提交 EXE。下载缓存关闭 Swatinem 的自动环境哈希，键只包含宿主与 Cargo.lock 中外部依赖摘要，项目版本和 CPU 变化不产生新下载缓存。交叉工作流在同一 Linux 宿主安装四个 Rust 目标，共用工具链缓存。

`.github/actions/setup-llvm` 为两个平台共用官方 LLVM 安装缓存和 apt 下载缓存。LLVM 仅在未命中时下载、验 SHA256、解压，随后裁剪为两平台共用的编译工具、共享库及 Clang 资源目录；安装验证成功即保存。不缓存 LLVM 调试器、分析工具和开发静态库。系统包先检查 runner 已安装内容，只补缺失项，安装失败时才刷新 apt 索引；Windows 签名工具仅在 Windows 任务安装。apt 下载缓存按 runner 镜像版本和包列表复用。SDK/CRT 由 `cargo xwin cache xwin` 独立准备并立即保存。macOS SDK 和 rcodesign 同样在安装后保存。缓存恢复有传输/解压开销，apt 仍需安装；闲置缓存淘汰见上面的部署清理流程。签名证书不进入缓存，项目与依赖每次重新编译。

macOS 默认从 `joseluisq/macosx-sdks` 社区清单自动选择最新 SDK 并校验摘要（可用仓库变量覆盖），配合官方 LLVM 的 Clang 与 Mach-O LLD，在 Linux 编译 Objective-C 桥接并打包。`packaging/macos/prepare-sdk.py` 校验 SDK 结构；`configure-cross.sh` 为目标配置编译器、归档器及链接器；`package-cross.py` 保持应用身份与 ZIP 布局，生成 PNG-backed ICNS，调用 rcodesign 签名并可选公证/装订。默认 ad-hoc 签名与原生工作流一致。配置变量、证书及工具限制见 [Linux macOS 打包](../../packaging/macos/README-cross.md)。

Windows x64 固定三个编译级别：兼容版 Rust `target-cpu=x86-64`、C `/clang:-march=x86-64`；AVX2 版两者使用 `x86-64-v3`；AVX-512 版两者使用 `x86-64-v4`。不使用 runner 的 `native`，兼容版遵循工具链的 Windows x64 基线且不要求 AVX／AVX2／AVX-512。Windows ARM64 和 macOS 保持各目标默认 CPU。release 的 O3、fat LTO 和单代码生成单元不变。

网页 `DownloadSection.tsx` 的 Windows x64 默认入口固定选择无后缀 AVX2 版，仅检测 OS／架构，不推断指令集。兼容／AVX-512 在选定 Release 存在对应附件时才显示为手动选项；选包提醒只说明一般用默认版、无法启动尝试兼容版；启动后任意包均可通过「检查更新」自动选择最高支持等级，同版本也可切换，无需手动核对指令集。`latest-release.ts` 按不同文件名尾部隔离六种附件，可选版不能覆盖默认 URL。

`KEYSTEER_CROSS_WINDOWS=1` 在非 Windows 宿主上启用 C 桥接和图标/manifest 编译，需要完整交叉工具链；未设置时保留不依赖 SDK 的跨平台类型检查。MSVC 交叉工作流显式设置 `RC_PATH=llvm-rc` 和空 `CROSS_COMPILE`，避免 winresource 0.1.31 初始化 GNU 前缀时误报 ARM64 未知目标。跨宿主的工具链、SDK、路径和签名时间戳可能改变哈希，不能据此断言行为不一致。

`KEYSTEER_CROSS_MACOS=1` 在非 Apple 宿主显式启用完整 Objective-C 桥接编译，需要 SDKROOT、Clang 与目标链接器；未设置时仍允许不安装 SDK 的跨平台类型检查。
