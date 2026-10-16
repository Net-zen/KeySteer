# 插件开发

插件是一个处理交互的 `Mode`，加上一份描述名称和调用方式的 `Manifest`。它与内置模式共用命令、事件和画面接口。

::: info 当前是内置插件
插件随 KeySteer 一起编译，在启动时注册。开发自己的插件可从 `src/plugins/builtin/screen_selector.rs` 开始；目前没有外部 DLL、dylib 或脚本加载接口，也不承诺跨版本 Rust API／ABI 兼容。
:::

## 核心概念

### 插件就是 Mode

实现 `api::Mode`，在 `handle` 中更新状态并返回 `CommandBatch`。以下摘录主要方法：

```rust
pub trait Mode: Send {
    fn id(&self) -> ModeId;            // 稳定标识，用于配置与唤醒
    fn display_name(&self) -> String { /* 默认: id 中的下划线替换为空格 */ }
    fn handle(&mut self, event: &ModeEvent, ctx: &HostContext<'_>) -> CommandBatch;
    fn captures_keyboard(&self) -> bool { true }              // 是否独占键盘
    fn claims_key(&self, _key: &Key) -> bool { false }        // 自己消费的原始字符
    fn wants_pointer_events(&self) -> bool { true }            // 是否接收高频指针事件
    fn indicator_color(&self, _palette: &Palette) -> Option<Color> { None }
}
```

系统操作交给宿主执行，插件不直接调用 Windows 或 macOS API。

### Manifest：告诉宿主你是谁

```rust
pub struct Manifest {
    pub id: String,            // reverse-DNS 风格唯一 id，如 "com.example.zoom"
    pub name: String,          // 人类可读名称
    pub description: String,   // 一句话说明
    pub default_chords: Vec<KeyChord>,     // 直接激活插件模式的遗留组合键
    pub verbs: Vec<String>,                // 插件导出的带参动词
    pub default_bindings: Vec<(KeyChord, Binding)>, // 建议绑定（见下）
}
```

用 `Manifest::new(id, name)` 创建后，可链式添加 `with_description`、`with_verb`、`with_chord`、`with_default_binding`。

注册时会校验名称：

| 字段 | 规则 |
| --- | --- |
| `id` | 非空，只含 ASCII 字母、数字、`.`、`_`、`-` |
| `verbs` | 每个动词非空，只含小写 ASCII 字母、数字、`_` |

### 动词（verb）：让用户在配置里调用你的插件

在 `Manifest.verbs` 声明动词后，用户即可在绑定中调用：

| 配置动作 | 插件收到什么 |
| --- | --- |
| `screen next` | 动词 screen，参数 next |
| `screen 2` | 动词 screen，参数 2 |
| `call screen` | 显式无参数调用 |

宿主通过 `ModeEvent::Invoked { verb, args }` 交付调用。完整的动作解析顺序见 [绑定解析顺序](/reference/modes-and-actions#绑定解析顺序)。

### 你能让宿主做什么（Command 精选）

| Command | 作用 |
| --- | --- |
| `MovePointer { dx, dy }` / `WarpPointer { x, y }` | 移动／跳转鼠标 |
| `MouseButton { button, action }` | 按下、松开或点击鼠标键 |
| `Scroll { dx, dy }` | 滚动 |
| `Command::show_overlay(OverlayScene)` / `HideOverlay` | 显示／隐藏覆盖层 |
| `SetFrameClock(true)` | 用系统显示帧驱动连续移动 |
| `SendKey` / `SendChord` | 向当前应用发送按键 |
| `Command::scan_ui(UiScanRequest)` | 请求异步识别，以 `UiScanned` 接收结果 |
| `SwitchMode(ModeId)` / `PushMode` / `PopMode` | 切换／压栈／弹出模式 |
| `RetargetScreen { index, preserve }` | 把会话移到指定显示器 |
| `SetTimer` / `CancelTimer` | 设置／取消延时任务 |
| `SetConfigValue` / `ReloadConfig` | 修改／重载配置 |
| `Exec { program, args }` | 后台启动程序 |
| `Quit` | 退出 KeySteer |

内置模式也使用这些命令。提交画面时，优先用 `ctx.present(View)` 交给统一展示层生成场景。

### 宿主会告诉你什么（ModeEvent 精选）

| ModeEvent | 时机 |
| --- | --- |
| `Activated { previous }` / `Deactivated` | 进入／离开；进入时可提交首帧 |
| `Pushed { previous }` / `Suspended` / `Resumed` | 压栈／暂停／恢复 |
| `Restarted` / `FinishRequested { cause }` | 从头开始／请求完成 |
| `Clicked { button, action }` | KeySteer 成功执行一次点击 |
| `Key { key, state, repeat }` | 原始按键，如标签或搜索输入 |
| `Binding { binding, state, key }` | 宿主已解析的配置动作 |
| `Invoked { verb, args }` | 插件动词调用 |
| `Frame { elapsed }` | 显示帧及实际经过时间 |
| `FocusChanged` / `ScreensChanged` / `ScreenRetargeted` | 焦点／屏幕变化 |
| `UiScanned(UiScanResult)` | 部分或完整识别结果 |
| `Timer { id, elapsed }` | 定时任务到期 |

::: tip 点击与保留状态的区别
一次成功的合成点击只发送一次 `Clicked`；物理点击、press/release/toggle 不发送。`keep` 保留模式实例和选择，只有 `restart` 发送 `Restarted`。
:::

### HostContext：只读的运行现场

`HostContext` 提供屏幕、鼠标位置、前台应用、调色板和 Presenter。`ctx.present(View)` 根据当前交互状态生成画面。

配置由 `src/app/mode_catalog.rs` 读取并转换为强类型设置，传给插件构造函数；`HostContext` 不提供配置文档或 `downcast_ref`。

## 配置集成：`[plugin_modes]`

插件模式名使用 `plugin:<id>`：

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

| 字段 | 用途 |
| --- | --- |
| `settings` | 插件专属值；装配时读取，如 `config.plugin_setting_bool(...)` |
| `bindings` | 插件按键表，支持继承和应用覆盖 |
| `inherits` | 继承其他模式的按键 |
| `temporary_mode` / `temporary_mode_keys` / `temporary_mode_passthrough_keys` | 临时模式、激活键、跳过临时层的键 |
| `app_configs` | 应用例外 |

::: info 插件没有 lifecycle 配置段
`PluginModeConfig` 未定义该字段。完成或点击后的行为，在插件的 `FinishRequested`、`Clicked` 或 `Binding` 处理中返回对应命令。
:::

## 注册插件

在 `src/app/mode_catalog.rs` 读取配置，转换为插件设置，再返回插件实例：

```rust
// src/app/mode_catalog.rs —— 装配点把 DTO 转成插件自己的 Settings。
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

编译 `RuntimePlan` 时，catalog 校验 Manifest 并注册模式和动词；失败会中止启动。

::: warning 建议绑定不会自动变成发布默认键
运行计划以用户配置为准，不注入 Manifest 建议。发布默认键位需同时写入配置默认值和 `keysteer.default.toml`。单独调用动态注册 API 时，建议只填补 Normal 的空位。
:::

## 内置示例：Screen Selector

完整代码在 `src/plugins/builtin/screen_selector.rs`。模式名为 `plugin:screen-selector`，动词为 `screen`：`next`／`previous` 切屏，数字选屏，`call screen` 打开编号选择器。

先声明 Manifest 和建议绑定：

```rust
let manifest = Manifest::new("com.keysteer.screen-selector", "Screen Selector")
    .with_description("Switch displays directly or choose one from a numbered overlay")
    .with_verb(VERB);
// primary+s 建议绑定为 "screen next"，不覆盖用户已有配置
let manifest = match KeyChord::parse_with_aliases("primary+s", aliases) {
    Ok(chord) => manifest.with_default_binding(chord, Binding::Invoke {
        verb: VERB.into(),
        args: vec!["next".into()],
    }),
    Err(_) => manifest,
};
```

再由装配点传入别名和设置：

```rust
Box::new(ScreenSelector::with_settings(
    &settings.key_aliases,
    settings.screen_selector_preserve,
)?)
```

插件通过 `ctx.present(...)` 与 `View::ScreenSelector` 展示编号，处理输入后返回 `RetargetScreen`。默认 `preserve = true`，Grid／Recursive Grid 切屏后保留选择路径。

## 规范与建议

- 插件依赖 `api`，新平台能力通过 API 扩展，系统调用放在 Backend。
- 优先复用 `move_left`、`left_click`、`finish` 等已有动作；`Binding` 事件已经完成按键解析。
- 用户配置优先，建议默认键只填空位。
- 插件专属值放在 `settings`；新增结构字段前先扩展 `PluginModeConfig`。
- 测试调用参数、取消、配置重载和资源释放；检查步骤见[开发流程与测试](/development/workflow)。
