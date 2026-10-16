# Workflow and tests

Run the app first, then choose checks for your change. See the [Extension guide](/en/development/extension-guide) if you are deciding where to work.

## Environment

| Purpose | Requirement |
| --- | --- |
| Rust development | Toolchain specified by `rust-toolchain.toml` |
| Documentation | Node.js 24, pnpm latest installed through npm |
| Native validation | Test on the target OS; macOS 14 minimum |

## Build and run

Run the development version from the repository root:

```bash
cargo run
cargo run -- --help
cargo run -- --doctor
cargo run -- --check -c ./keysteer.default.toml
cargo run -- --dump-config
```

| Option | Purpose |
| --- | --- |
| `--doctor` | Check keyboard, displays, foreground app, and permissions |
| `--check` | Validate configuration only |
| `--dump-config` | Print effective configuration |

::: tip Use ./ to check the repository configuration
A bare `--config` filename resolves in the app data directory. A relative path with a directory resolves from the working directory. Use `./keysteer.default.toml` for the repository example, or `.\keysteer.default.toml` in PowerShell. Absolute paths stay unchanged.
:::

Before submitting Rust changes, run:

```bash
cargo check
cargo test --all-features --lib --tests -- --test-threads=1
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

### Allocation and performance tests

Read each test's environment requirements first. Run allocation counters serially:

```bash
cargo test tests::performance::steady_normal_frames_do_not_allocate -- --ignored --exact --test-threads=1
cargo bench --manifest-path tools/perf/Cargo.toml --bench core_hot_paths
```

`tools/perf/` is a local tool requiring adapters in an isolated copy, not part of the published package. Do not add production APIs or features for benchmarks. See [Safety and performance constraints](/en/development/safety-and-performance).

## Platform packaging

Use scripts to produce complete packages with icons, signing, and resources:

```bash
# Windows
powershell -ExecutionPolicy Bypass -File packaging/windows/package.ps1

# macOS
./packaging/macos/package.sh
```

::: info macOS permissions follow app identity
Test the packaged `KeySteer.app` and follow [macOS permissions](/en/guide/macos), so permissions attach to the app rather than another host such as Terminal.
:::

## Documentation site

```bash
npm install --global pnpm@latest --allow-scripts=pnpm
pnpm install
pnpm docs:dev
pnpm docs:test
pnpm docs:check
pnpm docs:build
```

| Command | Purpose |
| --- | --- |
| `docs:dev` | Sync default TOML/icons and start a preview |
| `docs:test` | Test the configuration model and simulator |
| `docs:check` | Check Vue/TypeScript types |
| `docs:build` | Generate the static site and validate GitHub release metadata |

::: info Builds need release metadata
Development preview can continue when the network is unavailable. Production builds must fetch and validate GitHub Release metadata. Distinguish network/release errors from page errors when a build fails.
:::

The simulator previews and edits configuration. The app's `--check` and Rust tests determine final validity.

## Decide where to change code

| Need | Start with |
| --- | --- |
| Keys, speeds, themes, app exceptions | TOML |
| Several actions from one key | Action array |
| New session state or interaction flow | Mode |
| Reusable function with arguments | Bundled plugin + Manifest verb |
| OS capability | `api::Backend` and platform implementations |
| New configuration action syntax | Binding, Command, executor, tests, docs |

See the [Extension guide](/en/development/extension-guide) for steps.

## Change guidance

### Bindings or actions

1. Update parsing, normalization, and serialization in `src/api/binding.rs`.
2. Test valid/invalid input and array execution order.
3. Update default TOML, the [action reference](/en/reference/modes-and-actions), and simulator categories.
4. Check runtime command execution, configuration compilation, and integration tests.

### Modes

1. Confirm events, commands, and lifecycle semantics. Keep state in the Mode.
2. Request host capabilities through Command.
3. Test entry, exit, restart, and finish. Valid reloads should use new instances and return to Idle.
4. Check overlays, input release, and key ownership after transitions.

### Platform backends

1. Define a shared interface in `src/api/backend.rs`.
2. Implement Windows and macOS, and check unsupported.
3. Put expensive scans on workers without blocking the Engine.
4. Test hooks, permissions, overlays, injection, and multiple displays on the target OS.

## Test layers

| Test | Coverage |
| --- | --- |
| Rust unit tests | Bindings, configuration, mode state, geometry, routing |
| Internal integration in `src/tests/` | Shipped defaults, catalog, CLI, cross-platform behavior |
| Source guards in `tests/` | Dependencies, logging policy, safety budget; no internal-module imports |
| Documentation Node tests | Binding inheritance, configuration copies, simulator state |
| Native testing | Hooks, permissions, UIA/AX/Vision, overlays, status menus, packaging |

For Markdown-only changes, check links, anchors, and the commands below. Inspect a real preview when diagrams or layout change:

```bash
git diff --check
pnpm docs:check
pnpm docs:build
```

## Documentation maintenance

- Take defaults from source, tests, and `keysteer.default.toml`.
- Update architecture for data-flow changes; update user references and AI notes for configuration syntax changes.
- Maintain Chinese/English pages, chapters, examples, diagrams, and navigation together. Preserve old anchors when renaming headings.
- Explain purpose and steps first. Use info/tip/warning for reminders; reserve details for a few optional long examples.
