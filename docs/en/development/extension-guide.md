# Extending KeySteer

Use TOML for shortcuts, speeds, or action sequences. Add a Mode, plugin, or platform capability for a new interaction flow. Choose the entry point below for your change.

## Choose an approach

| Goal | Recommended approach | Main entry point |
| --- | --- | --- |
| Change keys, speed, themes, or an application's behavior | Configuration | `keysteer.<name>.toml` |
| Run several actions in sequence | Action array | `[<mode>.bindings]` |
| Add a keyboard workflow or targeting method | New Mode | `src/modes/` + `api::Mode` |
| Add a callable host capability | Bundled plugin | `src/plugins/` + `Manifest` |
| Add a cross-platform capability | Shared API + Backend | `src/api/`, `src/platform/` |
| Add a configuration action | Binding + Command | `src/api/binding.rs`, `src/api/command.rs` |

## The shortest development path

```text
Configuration change
  └─ TOML → --check → docs/tests

Mode or plugin
  └─ ModeEvent → state → Command → Engine → Backend

Platform capability
  └─ API trait → Windows/macOS/unsupported implementations → native tests
```

Read [Architecture](/en/development/architecture), then use [Workflow and tests](/en/development/workflow) for the checks relevant to your change.

## Add a Mode

A Mode receives events and read-only runtime context, updates its own state, and returns commands for the host.

1. Check whether the existing `ModeEvent` and `Command` types in `src/api/` cover the capability.
2. Add a state machine under `src/modes/`. Store session state there; keep Win32, AppKit, and Backend calls out of it.
3. Implement `api::Mode`, including `id`, `display_name`, and `handle`.
4. Add one catalog entry in `src/app/mode_catalog.rs`, converting loaded configuration into the Mode's `Settings`.
5. Test keys, completion, cancellation, configuration reloads, and failure cleanup.
6. Update the default TOML, user docs, sidebar, and AI maintenance notes together.

The core shape is:

```rust
impl Mode for MyMode {
    fn id(&self) -> ModeId {
        ModeId::new("my_mode").unwrap()
    }

    fn handle(&mut self, event: &ModeEvent, ctx: &HostContext<'_>) -> CommandBatch {
        // Update state; request host capabilities through commands.
        CommandBatch::new()
    }
}
```

::: tip Let the host perform system operations
Modes update state and return commands. The Engine/Backend handles pointers, windows, threads, and permissions so the same interaction logic can be tested on both platforms.
:::

## Add a plugin

::: info Plugins compile with the app
Bundled plugins register at startup. Developing one means changing the repository and rebuilding. See [Plugin development](/en/development/plugin-development) for the full interface.
:::

1. Implement `Mode` under `src/plugins/builtin/`.
2. Declare a stable ID, name, description, verbs, and suggested bindings in a `Manifest`.
3. Return the plugin from `bundled_plugins()` in `src/app/mode_catalog.rs`; the catalog validates its manifest.
4. Register the complete set through `RuntimePlan` during startup or reload, rather than assembling it piece by piece in bootstrap.
5. Configure inheritance, bindings, and plugin-specific `settings` under `[plugin_modes."plugin:<id>"]`. Use fields declared by `PluginModeConfig`; plugins do not inherit the built-in targeting modes' `lifecycle` section.
6. Test verb arguments, user binding precedence, reloads, and exit cleanup.

Plugins and built-in Modes use the same API:

```text
ModeEvent + HostContext → plugin state machine → CommandBatch
```

Add a shared API capability when needed, keeping plugins independent of Engine internals and platform modules.

## Add a configuration action

An action such as `notify <text>` needs changes in four places:

1. **Parsing:** add syntax, argument counts, and errors in `src/api/binding.rs`.
2. **Command:** add a platform-independent `Command` variant in `src/api/command.rs`.
3. **Execution:** handle it in the runtime command path under `src/app/runtime/mod.rs`, using `Backend` for native work.
4. **Tests and docs:** add unit and integration coverage, then update `keysteer.default.toml`, [Modes and actions](/en/reference/modes-and-actions), the simulator's action list, and AI maintenance notes.

Binding parsing rejects typos when configuration loads. Keep errors explicit so users can fix a binding before trying it at runtime.

## Add a platform capability

1. Define an interface in `src/api/backend.rs` that works for both Windows and macOS.
2. Implement it under `src/platform/windows/` and `src/platform/macos/`.
3. Check the `unsupported` implementation so unsupported platforms still compile and report a clear error.
4. Let the Engine coordinate scheduling, timeouts, and failure cleanup. Modes should not own native objects.
5. Test permissions, hooks, input injection, overlays, and multiple displays on the target OS.

## Before submitting

```bash
cargo fmt --check
cargo check
cargo test --all-features --lib --tests -- --test-threads=1
cargo clippy --all-targets --all-features -- -D warnings
cargo run -- --check -c ./keysteer.default.toml
```

For documentation or configuration-model changes, also run:

```bash
pnpm docs:check
pnpm docs:build
```

Check that examples can be copied, documented defaults match the code, and failures release keys, mouse buttons, overlays, timers, and other owned resources.
