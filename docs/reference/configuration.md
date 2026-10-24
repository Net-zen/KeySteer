# 配置文件

不改配置也能直接使用 KeySteer。想按自己的习惯改键、调速度或换颜色，可以编辑 TOML 文件；[完整默认配置可直接下载](/generated/keysteer.default.toml)。

::: tip 想边改边看效果？
先试[配置与模拟器](/editor/)。本文用于查字段和写法；动作名称与参数见[模式与动作参考](/reference/modes-and-actions)。
:::

## 配置文件位置

| 平台 | 数据目录 |
| --- | --- |
| Windows | 可执行文件所在目录 |
| macOS `.app` | `~/Library/Application Support/KeySteer/` |

在数据目录保存为 `keysteer.<名称>.toml`，例如 `keysteer.user.toml`。程序优先读取用户配置，其次是 `keysteer.default.toml`；都没有时使用内置默认值。

也可以指定文件：裸文件名从数据目录查找，`./` 开头的相对路径从当前工作目录查找。

```bash
keysteer --config keysteer.user.toml
keysteer --config ./profiles/keysteer.work.toml
keysteer --check --config keysteer.user.toml
```

| 命令 | 用途 |
| --- | --- |
| `keysteer --check -c keysteer.user.toml` | 校验配置，不启动控制 |
| `keysteer --dump-config` | 输出当前生效的完整配置 |
| `keysteer --doctor` | 检查键盘、显示器、权限和启动键 |
| `keysteer --help` | 查看命令行选项和默认快捷键 |

## 最小配置

下面展示“在 Normal 按空格退出”的写法。

::: warning 自定义按键表会替换默认表
未写的普通字段保留默认值，但 `[normal.bindings]` 等按键表会整体替换。建议复制完整默认配置再改键，保留需要的其他按键。用户配置与 default 文件也不会逐项合并。
:::

```toml
[normal.bindings]
# 在 Normal 中用空格返回 Idle
space = "idle"
```

保存后，在状态菜单选择 **Reload Configuration**。程序会回到 Idle，再按启动键进入 Normal。

想用点按切换速度，可把以下三行加入已有的 `[normal.bindings]`：

```toml
"1" = "precision_toggle"
"2" = "slow_toggle"
"3" = "fast_toggle"
```

再按同一个键恢复正常速度，按另一个键直接换档。倍率在 `[pointer]` 中设置；Grid 等模式仍优先把数字当作标签。

按住减速可写 `left_shift = "slow"`；点按切换则写 `shift = "slow_toggle"`。如果另有 `right_shift` 绑定，右 Shift 优先使用专用绑定。

## 配置结构

| 配置段 | 用来改什么 |
| --- | --- |
| `[general]` | 排除应用 |
| `[key_aliases]` | 自定义键名和跨平台修饰键 |
| `[hotkeys]` | 从 Idle 进入模式的启动键 |
| `[normal]`、`[grid]`、`[recursive_grid]`、`[ui_hint]` | 鼠标与定位模式的按键和行为 |
| `[window]`、`[window_quick]`、`[window_editor]`、`[window_restore]`、`[window_tab]` | 各窗口模式的按键和行为 |
| `[pointer]`、`[scroll]` | 鼠标速度、滚动距离和速度 |
| `[theme]`、`[mode_indicator]` | 颜色和模式提示 |
| `[key_help]` | 按键提示面板 |
| `[[app_configs]]` | 指定应用中的按键例外 |
| `[plugin_modes]` | 插件参数与按键 |
| `[debug]` | 排错日志 |

## 按键和别名

### 共享前缀的组合键

默认 `Primary+S` 切换鼠标所在显示器，`Primary+D` 移动窗口。若想共用前缀，也可以这样写：

```toml
[normal.bindings]
"primary+s" = "screen next"
"primary+s+d" = "move_window next"
```

- 按住 `Primary+S` 再按 `D`：移动窗口。
- 不按 `D`，直接松开短组合：切换鼠标显示器。

`move_window previous` 表示上一屏，`move_window 2` 表示指定屏幕。无需另设等待时间；切换模式或重载配置会取消尚未执行的短动作。

::: tip 连续操作用独立按键更顺手
共享前缀会让短组合等待松开；移动、长按点击和锁定按键建议使用独立绑定。
:::

### 按键写法

```toml
[normal]
long_press_toggle_ms = 500
auto_release_ms = 0

[normal.bindings]
h = "move_left"
"primary+shift+s" = "send primary+shift+s"
"v b" = "fast"
```

| 写法 | 含义 |
| --- | --- |
| `h` | 单键 |
| `primary+shift+s` | 同时按下的组合键 |
| `"v b"` | V 和 B 分别执行同一动作，空格不表示先后顺序 |
| `"mod+c enter"` | 组合键和单键也可同组；系统复制快捷键或 Enter 均执行该动作 |
| `ctrl` / `alt` / `shift` | 左右两侧都能触发 |
| `left_shift` / `right_shift` | 只匹配指定一侧 |

`primary` 是别名，默认 Windows 左 Alt、macOS Command。要在 Windows 改为 Ctrl，可设置 `[key_aliases.windows]` 中的 `Primary = "ctrl"`。

常用键名：`a-z`、`0-9`、`space`、`enter`、`esc`、`tab`、`delete`、`backspace`、`up/down/left/right`、`home/end`、`page_up/page_down`、`f1-f20`、`numpad_0-numpad_9`。

### 鼠标按键

Windows 和 macOS 都支持把以下五个鼠标按键放在绑定的左侧（触发）或右侧（点击）：

| 键名 | 通常对应 | 也可以写成 |
| --- | --- | --- |
| `mouse_left` | 左键 | — |
| `mouse_right` | 右键 | — |
| `mouse_middle` | 中键（按下滚轮） | — |
| `mouse_x1` | 后退侧键 | `xbutton1`、`mouse4` |
| `mouse_x2` | 前进侧键 | `xbutton2`、`mouse5` |

把需要的行合并到已有配置段，避免重复表名：

```toml
[hotkeys]
mouse_x1 = "normal"          # 从 Idle 进入工作模式

[normal.bindings]
mouse_x1 = "key_help"        # 在工作模式中切换按键提示
mouse_x2 = "grid"            # 进入网格定位
"ctrl+mouse_x2" = "ui_hint"  # Ctrl + 第二个侧键进入 UI Hint
```

所有鼠标按键都支持组合键、继承、应用例外和按住滚动。未绑定或设为 `none` 时保留原生行为；绑定后由 KeySteer 接管该次按键。

鼠标映射到键盘、键盘映射到鼠标以及鼠标之间的映射都可以：

```toml
[normal.bindings]
x = "mouse_left"       # X 触发左键点击
mouse_left = "x"       # 左键发送 X
mouse_middle = "mouse_right" # 中键触发右键点击
t = "mouse_x1"  # 模拟第一个侧键
y = "mouse_x2"  # 模拟第二个侧键
```

五个按键均支持 `press`、`release`、`toggle`，例如 `press mouse_left`、`release mouse_left`、`toggle mouse_middle`，以及普通点击的长按锁定。`mouse_left`、`mouse_right`、`mouse_middle` 作为动作时分别等价于 `left_click`、`right_click`、`middle_click`。侧键是否前进／后退由应用决定。若鼠标驱动已把按键改成键盘键，请绑定驱动实际输出的键。

### 自定义别名

别名让同一套配置适应不同键盘或平台：

```toml
[key_aliases]
Hyper = "right_ctrl"

[key_aliases.windows]
Primary = "left_alt"

[key_aliases.macos]
Primary = "left_cmd"
```

顶层别名在所有平台生效，平台段可分别指定。别名不区分大小写，值必须是一个键，不能是组合键；`Primary` 与 `primary` 是同一个别名。

### 绑定、数组和继承

动作可以写成一个字符串，也可以写成数组：

```toml
[normal.bindings]
h = "move_left"
x = ["press shift", "left_click", "release shift"]
"primary+shift+b" = ["exec say start", "wait 300", "exec say done"]
```

模式按键的优先级为：**自身绑定 → `inherits` 中按顺序列出的父模式**；当前应用的 `app_configs` 再覆盖结果。

```toml
[grid]
inherits = ["hotkeys", "normal"]

[grid.bindings]
q = "none" # 屏蔽从 normal 继承的 q
```

`none` 和 `__disabled__` 都表示禁用，可用来屏蔽继承的键。用户删除或改绑的插件快捷键不会被建议默认值补回。

::: tip 留一个启动键
至少保留一个 `[hotkeys]` 入口，否则程序虽然运行，却无法从 Idle 进入操作模式。
:::

### 动作序列

数组按从左到右执行，适合“按住修饰键 → 点击 → 松开”或启动脚本：

```toml
[normal.bindings]
x = ["press shift", "left_click", "release shift"]
"primary+shift+b" = ["exec say start", "wait 300", "exec say done"]
```

`wait` 只暂停当前序列，其他输入仍可处理；空数组不合法。完整动作和 `exec` 参数规则见 [模式与动作](/reference/modes-and-actions)。

## Normal 和定位 Mode

### Normal

Normal 用于移动、点击、滚动和进入其他模式。以下为部分配置：

```toml
[normal]
long_press_toggle_ms = 500
auto_release_ms = 0

[normal.bindings]
h = "move_left"
j = "move_down"
k = "move_up"
l = "move_right"
";" = "left_click"
g = "grid"
f = "recursive_grid"
"primary+f" = "ui_hint"
"primary+s" = "screen next"

# 可选：把 Primary+H/J/K/L 发送为应用的方向键。
# "primary+h" = "left"
# "primary+j" = "down"
# "primary+k" = "up"
# "primary+l" = "right"
```

| 参数 | 作用 |
| --- | --- |
| `passthrough_unbound_keys = true` | 默认：未绑定的键和组合键交给当前应用 |
| `passthrough_unbound_keys = false` | 独占键盘，并采用宽松的组合键匹配 |
| `long_press_toggle_ms` | 长按点击键后保持鼠标按下，默认 500 毫秒；0 关闭长按锁定 |
| `auto_release_ms` | 默认 0，手动释放；正数可在特定拖拽中停稳后自动松开 |

两个时间参数均为 `0–60000` 毫秒。自动松开只适用于“长按点击键＋已透传的物理修饰键”拖拽：移动后开始计时，后续移动重新计时；只松开鼠标，不松修饰键。`press`、`toggle` 锁定不参与。

无参数 `toggle` 可同时锁定多个伙伴键，先按伙伴或先按激活键都可以；单独短按激活键、返回 Normal 或进入 Idle 会全部释放。单独长按激活键会锁定它自身。具体步骤见 [拖拽与长按](/modes/normal#拖拽与长按)。

Grid、Recursive Grid、UI Hint 始终独占键盘；Idle 始终放行未命中的输入。

### Grid

```toml
[grid]
grid_cols = 5
grid_rows = 4
keys = "12345qwertasdfgzxcvb"
max_depth = 3
cursor_follow_selection = true

[grid.lifecycle]
after_finish = "normal"
after_click = "finish"
```

| 参数 | 作用 |
| --- | --- |
| `grid_cols` / `grid_rows` | 列数／行数 |
| `keys` | 从左到右、从上到下排列；字符数必须等于列数 × 行数 |
| `max_depth` | 选多少层后完成定位 |
| `cursor_follow_selection` | 选格时鼠标是否跟随 |

初始画面预览前两键，大字是第一键，小字是第二键。`[grid.ui]` 的 `matched_text_color`、`text_color`、`matched_border_color` 分别控制大字、小字基色和内部边界；预览不影响实际选择层数。

### Recursive Grid

```toml
[recursive_grid]
grid_cols = 3
grid_rows = 3
keys = "qweasdzxc"
max_depth = 10
min_size_width = 1
min_size_height = 1

[recursive_grid.lifecycle]
after_finish = "keep"
after_click = "keep"
```

`max_depth` 允许 `1–20`。`min_size_width` / `min_size_height` 控制最小区域尺寸。默认 `font_size = 0` 按格子自动缩放字号，正数（如 20）限制最大字号。

需要不同层用不同网格时，可设置 `layers`；未写的字段沿用基础设置：

```toml
[recursive_grid]
layers = [
  { depth = 0, grid_cols = 2, grid_rows = 2, keys = "crtn" },
]
```

### UI Hint

```toml
[ui_hint]
strategy = "hybrid" # axtree、vision、contour 或 hybrid
hint_characters = "asdfghjkl"
scan_timeout_ms = 2500
scan_retry_count = 1
scan_retry_delay_ms = 200
visible_check_enabled = false
clickable_roles = ["button", "link", "checkbox", "text_field", "menu_item"]

[ui_hint.lifecycle]
after_finish = "normal"
after_click = "normal"
```

| `strategy` | 识别来源 |
| --- | --- |
| `hybrid`（默认） | 辅助功能、所有可用 OCR 和轮廓 |
| `axtree` | 辅助功能信息 |
| `vision` | OCR 和轮廓；macOS 也使用 Vision 矩形检测 |
| `contour` | 轮廓，不运行 OCR 或辅助功能扫描 |

标签会分批出现。轮廓没有文字名称，不能按名称搜索；`clickable_roles` 只过滤辅助功能来源。

| 设置 | 说明 |
| --- | --- |
| `scan_timeout_ms` | 单次扫描超时 |
| `scan_retry_count` / `scan_retry_delay_ms` | 扫描结束或超时仍无标签时的重试次数／间隔 |
| `vision.request_timeout_ms` | 视觉和轮廓识别的请求超时 |
| `vision.rectangle_max_candidates` | 轮廓候选上限，默认 10000 |
| `vision.detect_text` / `vision.detect_rectangles` | 只控制 `vision` 来源；不关闭 `hybrid` 或独立 `contour` 的识别来源 |

扫描仍受范围、超时和总上限 10000 约束；高分辨率画面中的极小目标可能识别不到。Windows 使用本机系统 OCR；macOS 视觉识别需要录屏权限。

搜索用法见 [UI Hint](/modes/ui-hint#搜索与结果预览)。`search_match_priority = ["pinyin", "text", "label"]` 设置同等匹配程度下的优先级；`search_bindings` 的 `point_next` 默认 Tab。

当前标签高亮在 `[ui_hint.search_highlight]` 设置：`background_color`、`text_color`、`border_color` 支持浅深主题，`border_width` 为 0–20，默认 1.5；0 隐藏边框。未设置时使用内置绿色。

## Window 配置

第一次使用可先按场景练习：[Window 操作指南](/modes/window)。

五个配置段为 `[window]`、`[window_quick]`、`[window_editor]`、`[window_restore]`、`[window_tab]`。每个模式分别设置按键、继承、应用例外、临时模式、`ui` 和 `lifecycle`；默认只继承 hotkeys，按住 Primary 临时使用 Normal。

| 参数 | 用于哪里 | 默认值／作用 |
| --- | --- | --- |
| `move_step` / `move_speed` | Window | 移动步长 20／速度 600 |
| `resize_step` / `resize_speed` | Window、Editor，各自独立 | 缩放步长 20／速度 500 |
| `split_ratios` | Quick | `["1/4", "1/3", "1/2", "2/3", "3/4"]` |
| `gap` | Quick、Editor、Restore，各自独立 | 窗口间距，0 |
| `number_timeout_ms` | 五个模式 | 多位编号等待，250 毫秒；允许 100–2000 |
| `border_width` | 五个模式 | 窗口描边宽度，3 |
| `lifecycle.after_finish` | Restore | 恢复成功后进入 `"window_editor"` |

步长、间距为逻辑像素，速度为逻辑像素／秒；Windows 自动适应目标屏幕 DPI。编号样式用各模式的 `ui`，操作面板用 `[key_help]`。

下面示例添加直达启动键并分别修改参数；请合并到已有表中：

```toml
[hotkeys]
"alt+w" = "window"
"alt+a" = "window_quick"
"alt+e" = "window_editor"
"alt+r" = "window_restore"

[window_quick]
split_ratios = ["1/5", "2/5", "3/5", "4/5"]
gap = 12.0

[window_editor]
resize_step = 10.0
gap = 12.0

[window_restore.lifecycle]
after_finish = "window_editor"
```

应用例外可写在 `[[window_editor.app_configs]]` 等段中；共享按键通过 `inherits` 明确继承。按键的动作不随入口变化，例如 Editor 中配置 `q = "grid"` 就总是进入 Grid。

::: info 从旧窗口配置迁移
旧字段会报错并提示迁移，不会自动重写文件。按下面对应关系调整：
:::

| 旧写法 | 现在的写法 |
| --- | --- |
| `window_layout` / `window_edit` / `window_saved_layouts` | `window_quick` / `window_editor` / `window_restore` |
| 模式段 `[window_delete]` | 合并到 `[window_restore]`；`window_delete` 只是切换删除状态的动作 |
| `window_cancel` / `window_exit` | 直接写目标模式，如 `window` 或 `idle` |
| `window.exit_mode`、`layout_keys`、`double_tap_ms` | 删除 |
| `window.split_ratios` | 移到 `window_quick` |
| `window.gap` | 按用途移到 Quick／Editor／Restore |

## 指针、滚动和主题

```toml
[pointer]
initial_speed = 1000.0
max_speed = 2200.0
acceleration = 3000.0
smooth_acceleration = true
tap_distance = 2.5
slow_multiplier = 0.35
precision_multiplier = 0.12
fast_multiplier = 2.0

[scroll]
speed = 500.0
scroll_step = 50
scroll_step_half = 500
scroll_step_full = 1000000

[platform.macos.scroll]
invert_horizontal = false
invert_vertical = true
```

| 设置 | 改什么 |
| --- | --- |
| `initial_speed` / `max_speed` / `acceleration` | 起步速度、最高速度、加速度，单位像素／秒、像素／秒² |
| `smooth_acceleration` | true 起步和收尾更柔和；false 线性加速 |
| `tap_distance` | 短按移动距离 |
| `slow_multiplier` / `precision_multiplier` / `fast_multiplier` | 慢速、精确、快速倍率 |
| `scroll.speed` / `scroll_step` | 长按速度／短按距离；默认 500 像素／秒、50 像素 |
| `scroll_step_half` / `scroll_step_full` | 半页／整页滚动距离 |
| `invert_horizontal` / `invert_vertical` | macOS 横向／纵向滚动反转 |

普通滚动按住连续、松开停止；`speed = 0` 时仅短按。半页、整页滚动每次按下只执行一次。精确、慢速和快速键也影响滚动；速度不随显示器刷新率变化。

主题颜色用 `#RRGGBBAA`，末两位是透明度；可分别设置浅深主题：

```toml
[theme.dark]
surface = "#0A1338FF"
accent = "#6E82D6FF"
accent_alt = "#8FA2F0FF"
on_accent_alt = "#081022FF"
text = "#E8EEFFFF"

[mode_indicator.cursor]
left_pressed_color = "#00FF00FF"
middle_pressed_color = "#FF00FFFF"
right_pressed_color = "#00FFFFFF"
```

`*_pressed_color` 控制鼠标按住时的圆形提示，填充使用配置颜色 20% 的不透明度。模式标识符完整设置见 [Normal](/modes/normal#模式标识符样式与位置)。

## 应用覆盖：`[[app_configs]]`

为某个应用禁用或替换按键：

```toml
[[app_configs]]
bundle_id = "com.apple.Terminal"
bindings = { "primary+shift+e" = "none" }

[[normal.app_configs]]
bundle_id = "Figma"
bindings = { v = "none", "primary+f" = "grid" }
```

根级 `[[app_configs]]` 对所有模式生效，`[[normal.app_configs]]` 只影响 Normal。`bundle_id` 可写 macOS bundle id、Windows 可执行文件名，或窗口标题子串。

## 插件设置

插件模式名使用 `plugin:<id>`：

```toml
[plugin_modes."plugin:screen-selector".settings]
preserve = true

[plugin_modes."plugin:screen-selector"]
inherits = ["hotkeys", "normal"]
```

Screen Selector 的 `preserve = true` 会在切屏时保留 Grid／Recursive Grid 的选择路径；false 则在新屏幕重新开始。开发自己的插件见[插件开发](/development/plugin-development)。

## 运行时修改与调试

用 `set_config` 修改点号分隔的字段路径，校验通过后保存到当前配置文件：

```toml
[normal.bindings]
"primary+1" = "set_config pointer.max_speed 800"
"primary+2" = "set_config theme.dark.accent \"#FF8800FF\""
```

无效修改会保留原来的有效配置。手动编辑文件后，可从状态菜单重载。

需要排错时，临时开启日志类别：

```toml
[debug]
enabled = true
keys = true
actions = true
modes = true
backend = true
pointer = false
motion = false
overlay = true
timers = true
```

日志位于数据目录的 `keysteer.log`。排查结束后关闭调试日志，避免持续产生大量记录。

## 实时按键提示

按 `?` 显示或隐藏当前模式的操作提示，不会清空选择。已有自定义绑定表需保留 `"?" = "key_help"`；问号按系统实际输入的字符匹配。

| 设置 | 影响哪些模式 | 默认 |
| --- | --- | --- |
| `mouse_key_help` | Normal、Grid、Recursive Grid、UI Hint | false |
| `window_key_help` | Window、Quick、Editor、Restore、Tabs | true |

例如，想让窗口面板也默认隐藏：

```toml
[key_help]
mouse_key_help = false
window_key_help = false
```

手动显示状态在同组模式间保持；回到 Idle 后重新进入恢复默认。隐藏帮助不会隐藏窗口编号或边框。旧 `[key_help].enabled` 已改名为 `mouse_key_help`。

`[key_help]` 支持 `font_family`、`font_size`、`background_color`、`text_color`、`border_color`、`border_width`、`border_radius`、`padding_x`、`padding_y`。未设置的字体和颜色跟随模式提示及主题；颜色支持浅深主题表。可在[模拟器](/simulator)点击“编辑按键提示样式”预览。

Window 的编号字号用 `[window.ui].font_size`，默认 28。Quick 比例尺可混用分数和小数；要保留尾零可写字符串 `"0.30"`，屏幕预览会显示当前宽高比例。

### 窗口卡片引导线

编号卡片避让后，会用引导线指向对应窗口。关闭线条不影响卡片避让。

```toml
[window.card]
guide_line_enabled = true
guide_line_width = 3.0 # 0–32；0 也隐藏线条
# 默认继承卡片边框颜色；末两位是透明度，也支持浅深主题颜色。
# guide_line_color = "#6E82D680"
# 内置选中样式为柔和薄荷绿；以下为可选覆盖，默认无需配置。
# selected_background_color = { light = "#E8F6F0FF", dark = "#284D44FF" }
# selected_border_color = { light = "#60B49CFF", dark = "#85CDB8FF" }
# selected_border_width = 1.5 # 0–20；0 隐藏选中卡片边框
```

线宽允许 0–32，0 隐藏线条；选中边框宽度允许 0–20，0 隐藏边框。颜色支持单个 `#RRGGBBAA` 或浅深主题表。

在 **窗口管理 → 共用外观** 可预览普通／选中卡片。修改后导出对应字段，重置可恢复内置绿色样式。

## 模式统计与快速切换

单独长按 `Q`，默认 350 毫秒后进入 `quick_switch` 模式，松开后保持显示。进入后输入标签筛选并选择，默认字符为 `wertyuiop`。它使用普通模式的生命周期与绑定，也可以通过普通绑定进入。

```toml
[mode_usage]
save_after_entries = 100

[quick_switch]
enabled = true
key = "q"
hint_characters = "wertyuiop"
hold_ms = 350
blacklist = ["idle"]
position = "mouse" # screen / window / mouse

[quick_switch.bindings]
esc = "cancel_selection"
q = "cancel_selection"
backspace = "previous_level"

[quick_switch.ui]
font_size = 28
border_width = 1
border_radius = 6
padding_x = 6
padding_y = 8
# background_color = "#EEF2FFFF"
# text_color = "#193781FF"
# border_color = "#506DD0FF"
```

| 设置 | 作用 |
| --- | --- |
| `save_after_entries` | 每新增多少次模式进入记录后保存，必须大于 0 |
| `enabled` / `key` / `hold_ms` | 开关、长按键、展开等待时间 |
| `hint_characters` | 按此顺序生成标签，至少两个不重复的字符，不可包含触发键；也可设为 `123456789` |
| `blacklist` | 从面板隐藏指定模式；原快捷键仍可使用 |
| `position` | `screen` 屏幕中心、`window` 前台窗口中心、`mouse` 鼠标提示下方 |
| `quick_switch.bindings` | 普通按键绑定；`cancel_selection` 返回，`previous_level` 删除一位前缀 |
| `quick_switch.ui` | 字体、字号、颜色、边框、圆角和内边距 |

面板按使用次数排序，同分按模式 id 排序，不再限制为 9 项。复用 UI Hint 的普通标签算法：9 项以内使用单字符，更多项逐步使用无歧义的多字符标签，例如 10 项为 `w e r t y u i o pw pe`。输入前缀会筛选，完整标签立即切换；默认 Backspace 删除一位，Esc 或再次按 Q 退出并返回进入前的模式；松开触发键不退出。打开后标签固定，筛选不重新编号；候选不包含快速切换自身和进入前的模式。过长列表会按屏幕高度分列。前台窗口不可用时，`window` 回退到屏幕中心，面板始终限制在当前屏幕内。

配置 `[quick_switch.bindings]` 时整张表替换默认值：只写 `esc = "cancel_selection"` 即只允许 Esc；只写 `q = "cancel_selection"` 即只允许 Q；两行都写则都能退出。需要退格时保留 `backspace` 行。绑定键不能与标签字符冲突。

已输入的标签前缀默认使用与 UI Hint 相同的黄色高亮；通过 `quick_switch.ui.matched_text_color` 修改，支持浅／深主题颜色。`Quick Switch` 模式指示器沿用共享样式和主题强调色。

::: info Q 仍先执行原来的动作
按下 Q 会立即执行原绑定；进入快速切换后，不再重复原动作，松开时只正常结束原手势，面板保持。即使原动作已进入 Idle，本次长按仍有效；从 Idle 开始、暂停、排除应用或文本／备注输入时不接管 Q。
:::

统计只记录实际模式切换，同模式保持或重启不增加次数。记录保存在 `workspace.ksw`；保存／删除预设和正常退出时也会保存。强制结束或断电可能丢失最近未保存的次数。

网页可导入工作区查看统计并修改面板设置；导出保留统计。带统计的 v2 工作区需要支持该格式的程序。

## 搜索面板定位

搜索框与信息面板都用“参照对象＋四边百分比”定位：

```toml
[ui_hint.search_input_ui]
position_mode = "screen"
position = ["95%", "50%", "5%", "50%"]

[ui_hint.search_info_ui]
position_mode = "search_input"
position = ["44%", "50%", "56%", "50%"]
```

| `position_mode` | 参照哪里 |
| --- | --- |
| `screen` | 鼠标所在屏幕 |
| `window` | 识别窗口；没有窗口则用当前屏幕 |
| `search_input` | 搜索框；只用于信息面板，让它跟随搜索框 |

`position` 依次为**上、右、下、左边距**，均为 `0%–100%`；相对两边之和不能超过 100%。屏幕和窗口定位使用所选区域的中心点，面板大小另由宽度、字体和内边距决定。

| 信息面板相对搜索框的位置 | `position` |
| --- | --- |
| 上方留少量间距（默认） | `["44%", "50%", "56%", "50%"]` |
| 紧贴上方 | `["50%", "50%", "50%", "50%"]` |
| 下方留相同间距 | `["56%", "50%", "44%", "50%"]` |

默认搜索框中心位于屏幕高度 95%。在网页外观页点选、拖动范围或调整滑块，可同时预览两个面板的位置和间距。旧 `x_offset`／`y_offset` 不再生效，请改用 `position`。

## 临时模式按键优先级

按住 `temporary_mode_keys` 时，依次查找：

1. 临时模式的完整组合键。
2. 当前模式的完整组合键。
3. 去掉临时激活键后的临时模式绑定。
4. 去掉临时激活键后的当前模式绑定或标签输入。

每一层都包含其继承绑定。完整组合键优先于裸键，例如 Normal 的 `Primary+F` 进入 UI Hint，优先于裸 F 的 Recursive Grid；Grid 的 `Primary+Q` 仍返回 Normal。

想让 Q 直接交给当前模式，可声明穿透键：

```toml
[grid]
temporary_mode = "normal"
temporary_mode_keys = ["primary"]
temporary_mode_passthrough_keys = ["q"]

[grid.bindings]
q = "normal"
```

穿透键只跳过临时模式，不向系统发送按键。默认 `[]`，支持组合键、别名和左右修饰键；同样适用于其他定位模式、窗口模式和插件。

::: info Window 的临时键有不同规则
Window 先保留自身显式组合键，再去掉激活键查找 Normal 和当前模式。例如临时 S 可切屏，临时 F 默认进入 Recursive Grid。窗口面板显示窗口操作，不会列出整张 Normal 按键表。
:::

## Normal 盲操定位

Normal 也能直接输入网格键定位，但默认关闭。启用步骤和安全示例见 [Normal 盲操网格定位](../modes/normal.md#盲操网格定位)。

| `[normal.targeting]` 字段 | 作用 |
| --- | --- |
| `method` | `grid`（默认）或 `recursive_grid`，沿用对应网格的布局 |
| `reset_on` | 默认 `["move", "click"]`，移动或 KeySteer 点击后重选；可取子集或 `[]` |
| `grid_cols` / `grid_rows` / `keys` / `max_depth` | 只在 Normal 中覆盖布局，不改变独立网格模式 |
| `min_size_width` / `min_size_height` / `layers` | 仅 `recursive_grid` 可用；显式 layers 整组替换，`[]` 清空 |

未写的字段沿用对应网格。此配置不接受 UI 样式；键位冲突会在加载时报告。删除整个段即可关闭，完整注释见 `keysteer.default.toml`。

## 文字选择配置

`text_select` 复用 UIHint 的识别与外观，单独配置入口、标签和操作键。Normal 默认用 `c` 进入。以下是默认绑定；显式填写该绑定表会替换文字选择的默认表。

```toml
[text_select]
enabled = true
hint_characters = "asdfghjklqwertyuiopzvbnm"

[text_select.lifecycle]
after_copy = "normal"

[text_select.bindings]
"alt+c" = "text_refine" # macOS Option+C，Windows Alt+C
"alt+backspace" = "previous_level"
"ctrl+backspace" = "first_level"
tab = "text_cycle_page"

"ctrl+a" = "text_select_all"
backspace = "text_clear_selection"
"ctrl+x" = "text_clear_input"
"mod+c enter" = "text_copy"

"primary+q" = "normal"
esc = "cancel_selection"
"?" = "key_help"
```

`[text_select.ui]` 的所有字段都有内置默认值，整段可省略；自定义时只需写要修改的字段。包围框样式和鼠标偏移可在网页配置编辑器的 **文字选择 → 外观** 中调节。边距与鼠标落点使用交互示意图：拖动包围框右下角调整边距，切换到鼠标落点后点击或拖动定位；也可使用横向／纵向滑块和方向键微调，Shift 加速、Esc 撤销当前拖动。

`panel_width` 设置本模式面板宽度，默认 800，范围 320–2400；实际显示受屏幕／窗口宽度限制。数量统计下方有留白和分隔线，卡片内标签与文字紧凑排列。页脚显示当前页／总页数及实际翻页键；Tab 循环翻页，末页回到首页，空输入时 Backspace 清空勾选，Alt+Backspace 返回上一层。

单词卡片按各自长度自动排满一行，长词不会加宽其他卡片；字符层也会自动换行。`word_min_width`／`char_min_width` 分别设置最小宽度，默认 48／40，范围 24–240；调小可显示更多列，调大可增加间距。两项都在“文字选择 → 外观”中调节。屏幕高度不足时会压缩过宽卡片并省略显示长词，复制内容保持完整。

`pointer_offset = [x, y]` 是鼠标相对原始区域右下角的偏移，默认 `[12, 12]`，正值向右／向下。包围框字段只控制第一层：`border_color` 省略时跟随主题强调色，支持浅深主题；`border_width` 为 0–20，0 隐藏；`padding` 使用单个 `[x, y]` 数组，向水平／垂直两侧各外扩 0–100，默认 `[2, 2]`。

`text_select.lifecycle.after_copy` 仅在复制勾选文字成功后执行，默认 `normal`。可设为 `keep` 保留当前选择、`restart` 重新识别、`return` 返回进入前的模式，或其他内置／已配置插件模式；不接受点击动作或 `finish`。复制失败保留当前选择。初始只显示输入框和原位标签，输入后才显示紧凑文字预览；细分时保留预览。

升级时若没有 `[text_select]` 或 `[text_input]`，模式及内部按键采用默认值；旧 Normal 绑定表缺少对应入口时补齐 `c`／`\`，但不覆盖已占用、参与自定义组合键／Normal 盲操网格或设为 `none` 的按键，也不追加已改键模式的第二个入口。显式 `text_select.enabled = false` 仍关闭文字选择。

进入模式的键在 Normal 配置，细分键在 Text Select 配置，互不影响。第一层直接输入文字、简拼或空格分隔的标签；词语／字符层默认全选，标签固定为数字，`hint_characters` 仅控制第一层。字母／文字／简拼搜索当前细分层的全部条目（跨页），只筛选候选并保留已有勾选；单个编号切换勾选，`1-20`、`1-3 4-5` 勾选区间，按输入顺序组合，无歧义编号立即生效，其余用空格或复制／细分动作确认；普通 `c` 始终参与输入，`Alt+C` 细分（macOS 为 Option+C，不依赖 Primary 别名），网页“文字选择 → 按键”使用统一按键映射编辑“复制文字”（`text_copy`）和“清空输入框”（`text_clear_input`）；默认分别为 `mod+c enter` 和 `ctrl+x`。Ctrl+C（macOS Command+C）和 Enter 始终复制面板勾选文字，与输入内容和选区无关；Ctrl+X 清空整个输入框，不操作剪贴板，不改变当前层级或词语／字符层已确认的勾选。`Primary+Q` 不复制并返回 Normal。已有配置请将旧细分绑定替换为 `alt+c = "text_refine"`。输入框中 Space 用于输入；Backspace 在输入非空时编辑，空输入时清空勾选；Ctrl+A 有输入时全选输入，空输入时全选勾选；Alt+Backspace 返回上层。UIHint 的 `search_edit_keys` 同时提供光标、粘贴等编辑键；文字选择操作绑定优先。绑定为操作的单字母不会生成区域层标签。

原来不能修改的网格、UIHint、屏幕选择和窗口导航键也已开放配置，旧默认键保持不变；稀疏旧配置自动补齐这些原有操作，显式 `none` 可禁用。详见[可配置的选择操作](/reference/modes-and-actions#可配置的选择操作)。
