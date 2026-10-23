# Plugin development

A plugin combines a `Mode` that handles interaction with a `Manifest` describing its name and calls. It shares commands, events, and presentation interfaces with built-in modes.

::: info Plugins are bundled today
Plugins compile with KeySteer and register at startup. Start with `src/plugins/screen_selector.rs`. External DLL, dylib, and script loading is not available, and Rust API/ABI compatibility across versions is not guaranteed.
:::

## Core concepts

### A plugin is a Mode

Implement `api::Mode`. Update state in `handle` and return a `CommandBatch`. These are the main methods:

```rust
pub trait Mode: Send {
    fn id(&self) -> ModeId;
    fn display_name(&self) -> String { /* ID with underscores replaced by spaces */ }
    fn handle(&mut self, event: &ModeEvent, ctx: &HostContext<'_>) -> CommandBatch;
    fn captures_keyboard(&self) -> bool { true }
    fn claims_key(&self, _key: &Key) -> bool { false }
    fn wants_pointer_events(&self) -> bool { true }
    fn indicator_color(&self, _palette: &Palette) -> Option<Color> { None }
}
```

The host performs system operations. Plugins do not call Windows or macOS APIs directly.

### Manifest: describe the plugin

```rust
pub struct Manifest {
    pub id: String,             // Stable ID, e.g. "com.example.zoom".
    pub name: String,           // Display name.
    pub description: String,    // One-sentence description.
    pub default_chords: Vec<KeyChord>, // Legacy direct activation suggestions.
    pub verbs: Vec<String>,            // Exported verbs with arguments.
    pub default_bindings: Vec<(KeyChord, Binding)>, // Suggested bindings.
}
```

Start with `Manifest::new(id, name)`, then chain `with_description`, `with_verb`, `with_chord`, and `with_default_binding`.

Registration validates names:

| Field | Rule |
| --- | --- |
| `id` | Nonempty; ASCII letters, digits, `.`, `_`, `-` only |
| `verbs` | Each nonempty; lowercase ASCII letters, digits, `_` only |

### Verbs: call a plugin from configuration

Declare verbs in `Manifest.verbs` so users can call them from bindings:

| Configured action | Plugin receives |
| --- | --- |
| `screen next` | Verb screen, argument next |
| `screen 2` | Verb screen, argument 2 |
| `call screen` | Explicit call with no arguments |

The host delivers `ModeEvent::Invoked { verb, args }`. See [Binding parsing order](/en/reference/modes-and-actions#binding-parsing-order) for full resolution rules.

### What the host can do: selected commands

| Command | Purpose |
| --- | --- |
| `MovePointer { dx, dy }` / `WarpPointer { x, y }` | Move / jump the pointer |
| `MouseButton { button, action }` | Press, release, or click a mouse button |
| `Scroll { dx, dy }` | Scroll |
| `Command::show_overlay(OverlayScene)` / `HideOverlay` | Show / hide an overlay |
| `SetFrameClock(true)` | Use display frames for continuous movement |
| `SendKey` / `SendChord` | Send keys to the focused app |
| `Command::scan_ui(UiScanRequest)` | Request asynchronous recognition; receive `UiScanned` results |
| `SwitchMode(ModeId)` / `PushMode` / `PopMode` | Switch / push / pop modes |
| `RetargetScreen { index, preserve }` | Move the session to a display |
| `SetTimer` / `CancelTimer` | Schedule / cancel delayed work |
| `SetConfigValue` / `ReloadConfig` | Change / reload configuration |
| `Exec { program, args }` | Start a program in the background |
| `Quit` | Exit KeySteer |

Built-in modes use the same commands. Prefer `ctx.present(View)` to build scenes through the shared presentation layer.

### What the host reports: selected events

| ModeEvent | When it arrives |
| --- | --- |
| `Activated { previous }` / `Deactivated` | Entering / leaving; submit the first scene on entry |
| `Pushed { previous }` / `Suspended` / `Resumed` | Push / suspend / resume |
| `Restarted` / `FinishRequested { cause }` | Start over / request completion |
| `Clicked { button, action }` | One successful click made by KeySteer |
| `Key { key, state, repeat }` | Raw keys, such as labels or search input |
| `Binding { binding, state, key }` | A configuration action resolved by the host |
| `Invoked { verb, args }` | A plugin verb call |
| `Frame { elapsed }` | A display frame and actual elapsed time |
| `FocusChanged` / `ScreensChanged` / `ScreenRetargeted` | Focus / display changes |
| `UiScanned(UiScanResult)` | Partial or complete recognition results |
| `Timer { id, elapsed }` | Delayed work is due |

::: tip Clicks and retained state
One successful synthetic click emits one `Clicked`. Physical clicks and press/release/toggle do not. `keep` preserves the mode instance and selection; only `restart` sends `Restarted`.
:::

### HostContext: read-only runtime context

`HostContext` supplies displays, pointer position, focused app, palette, and Presenter. `ctx.present(View)` builds a scene from current interaction state.

`src/app/mode_catalog.rs` reads configuration and passes typed settings to plugin constructors. `HostContext` has no configuration document or `downcast_ref` entry.

## Configuration: `[plugin_modes]`

Plugin mode names use `plugin:<id>`:

```toml
[plugin_modes."plugin:screen-selector".settings]
preserve = true

[plugin_modes."plugin:screen-selector".bindings]
"1" = "left_click"
esc = "escape"

[plugin_modes."plugin:screen-selector"]
inherits = ["hotkeys", "normal"]
temporary_mode = "normal"
temporary_mode_keys = ["primary"]
```

| Field | Purpose |
| --- | --- |
| `settings` | Plugin values read during assembly, for example with `config.plugin_setting_bool(...)` |
| `bindings` | Plugin keys, with inheritance and app overrides |
| `inherits` | Inherit keys from other modes |
| `temporary_mode` / `temporary_mode_keys` / `temporary_mode_passthrough_keys` | Temporary mode, activation keys, and keys that skip its layer |
| `app_configs` | App exceptions |

::: info Plugins have no lifecycle section
`PluginModeConfig` does not define it. Return commands from `FinishRequested`, `Clicked`, or `Binding` handlers for completion or click behavior.
:::

## Register a plugin

In `src/app/mode_catalog.rs`, read configuration, convert it into plugin settings, then return plugin instances:

```rust
// src/app/mode_catalog.rs
pub(crate) fn bundled_plugin_settings(config: &Config) -> BundledSettings {
    BundledSettings {
        key_aliases: config.resolved_key_aliases().clone(),
        screen_selector_preserve: config
            .plugin_setting_bool("plugin:screen-selector", "preserve")
            .unwrap_or(true),
    }
}

pub fn bundled_plugins(config: &Config) -> Result<Vec<Box<dyn Plugin>>, String> {
    crate::plugins::bundled(bundled_plugin_settings(config))
}
```

When compiling `RuntimePlan`, the catalog validates manifests and registers modes and verbs. Failure stops startup.

::: warning Suggestions do not become shipped defaults automatically
The runtime plan uses user configuration without injecting Manifest suggestions. Shipped keys belong in configuration defaults and `keysteer.default.toml`. When calling the dynamic registration API separately, suggestions fill only unused Normal keys.
:::

## Bundled example: Screen Selector

The complete code is in `src/plugins/screen_selector.rs`. Its mode name is `plugin:screen-selector` and its verb is `screen`: `next`/`previous` switches displays, a number selects one, and `call screen` opens the numbered selector.

First declare the Manifest and suggested binding:

```rust
let manifest = Manifest::new("com.keysteer.screen-selector", "Screen Selector")
    .with_description("Switch displays directly or choose one from a numbered overlay")
    .with_verb(VERB);
// Suggest primary+s = "screen next" without replacing user bindings.
let manifest = match KeyChord::parse_with_aliases("primary+s", aliases) {
    Ok(chord) => manifest.with_default_binding(chord, Binding::Invoke {
        verb: VERB.into(),
        args: vec!["next".into()],
    }),
    Err(_) => manifest,
};
```

Then pass aliases and settings from the assembly point:

```rust
Box::new(ScreenSelector::with_settings(
    &settings.key_aliases,
    settings.screen_selector_preserve,
)?)
```

The plugin presents numbers with `ctx.present(...)` with `View::ScreenSelector` and returns `RetargetScreen` after input. The default `preserve = true` keeps Grid/Recursive Grid selection paths across displays.

## Conventions and advice

- Depend on `api`. Add new platform capabilities there and put system calls in the Backend.
- Reuse actions such as `move_left`, `left_click`, and `finish`. `Binding` events already contain resolved keys.
- User configuration comes first; suggested keys only fill gaps.
- Put plugin values in `settings`. Extend `PluginModeConfig` before adding structural fields.
- Test arguments, cancellation, reloads, and cleanup. See [Workflow and tests](/en/development/workflow) for checks.
