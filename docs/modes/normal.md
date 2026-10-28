# Normal 普通模式

<script setup>
import ModeVideo from '../.vitepress/components/ModeVideo'
import KeyLayout from '../.vitepress/components/KeyLayout'
</script>

日常移动、点击、滚动和拖拽，都从 Normal 开始。按 `Primary+E` 进入，按 `Esc` 或 `Q` 回到待机。

`Primary` 默认是 Windows 的左 Alt、macOS 的 Command。Normal 只使用已配置的按键，其余按键照常交给当前应用。

<ModeVideo
  file="normal.mp4"
  title="Normal 模式演示"
  description="展示键盘移动鼠标、速度修饰、滚动和常用点击操作。"
/>

## 默认按键

<KeyLayout
  layout="q w e r t y u i o p/Caps a s d f g h j k l ; '/Shift z x c v b n m , . Slash RShift/Ctrl Primary Alt Space"
  move="h j k l"
  click="; ' RShift"
  speed="Caps Shift v b"
  scroll="m ,"
  state="n"
  navigation="t y u i"
  mode="e f g q Primary"
  label="Normal 默认键位分区"
  hint="颜色表示动作类型；空白键未占用"
/>

| 按键 | 作用 |
| --- | --- |
| `H / J / K / L` | 向左 / 下 / 上 / 右移动鼠标 |
| `Caps Lock` / `左 Shift` | 精确 / 慢速移动 |
| `V` 或 `B` | 快速移动 |
| `M` / `,` | 向下 / 向上滚动 |
| `;` / `'` / `右 Shift` | 左键 / 右键 / 中键点击 |
| `N` + 点击键 | 锁住鼠标按钮，方便拖拽；单独点 `N` 松开 |
| `T / Y / I / U` | Home / End / Page Up / Page Down |
| `G` / `F` / `Primary+G` / `Primary+F` | Grid / UI Hint / Recursive Grid / Bisect |
| `Primary+S` | 将鼠标切到下一块显示器 |
| `Primary+D` | 将鼠标下的窗口移到下一块显示器 |
| `Q` / `Esc` | 返回待机 |

::: tip 不用全记住
按 `?` 查看当前可用按键，再按一次收起。字母大写只是为了好认；写出 `Shift+` 时才需要按 Shift。
:::

## 盲操网格定位

记住网格位置后，可以直接按格子的键跳转，再用 `H/J/K/L` 微调。整个过程留在 Normal，屏幕上不显示网格。

::: info 可选功能，默认关闭
下面用四块区域示范：`1` 左上、`2` 右上、`3` 左下、`4` 右下。连续按 `2 → 3`，就是先到屏幕右上，再到那一块的左下。
:::

将这段加入配置即可试用。先确认 `1/2/3/4`、`Tab`、`Backspace`、`Space` 没有绑定其他动作；发布默认配置中这些键都是空闲的。

```toml
[normal.targeting]
method = "grid"
grid_cols = 2
grid_rows = 2
keys = "1234"
max_depth = 3
reset_on = ["move", "click"]
```

| 操作 | 会发生什么 |
| --- | --- |
| 继续按格子的键 | 在选中的区域里继续细分，鼠标跳到小格中心 |
| `Tab` / `Backspace` | 退回上一层，鼠标留在原处 |
| `Space` | 从整屏重新选，鼠标留在原处 |
| 选到最后一层 | 留在当前位置，仍可微调、点击或回退 |

上面的配置会在 `H/J/K/L` 微调或 KeySteer 点击后，从整屏开始下一次选格。想保留当前区域，改成 `reset_on = []`。实体鼠标操作和滚动不触发重选，`Esc` 和点击键仍照常使用。

想换用已有 Grid 或递归网格布局，见[盲操定位配置](/reference/configuration#normal-盲操定位)。删除 `[normal.targeting]` 段即可关闭；只留下空表会启用默认网格。

## 移动速度

小目标不好点时，按住 `Caps Lock` 精确移动；想慢一点用左 `Shift`，想快一点用 `V` 或 `B`。松开后恢复原速度，这些键也影响滚动。

::: tip 不想一直按住？可以点按换挡
把下面三行加入已有的 `[normal.bindings]`。点一次换挡，再点同一个键恢复正常速度；当前档位显示在模式名下方。

```toml
f5 = "precision_toggle"
f6 = "slow_toggle"
f7 = "fast_toggle"
```
:::

想整体调快或调慢，可在[配置与模拟器](/editor/)的 **Normal → 行为** 中调整。直接编辑文件时，速度和倍率写在 `[pointer]`，见[完整速度配置](/reference/configuration#指针、滚动和主题)。

## 拖拽与长按

拖动窗口或文件时，可以先锁住鼠标左键，再腾出手来移动：

1. 将鼠标移到窗口标题栏或要拖动的文件上。
2. 按住 `;` 约半秒，然后松开，左键会继续保持按下。
3. 用 `H/J/K/L` 拖到目标位置。
4. 单独点一下 `N`，松开左键，完成拖拽。

::: tip 也可以用 N 配合点击键
按住 `N`，点一下 `;`，再松开 `N`，也能锁住左键。先按点击键再按 `N` 也可以。右键和中键同理，分别用 `'` 和右 `Shift`。
:::

`N` 也能配合其他按键，让多个键保持按下。单独点 `N`、切回 Normal 或回到待机时，会松开所有已锁住的键和鼠标按钮。

想改长按时间，可以调整：

```toml
[normal]
long_press_toggle_ms = 500 # 半秒；设为 0 关闭长按锁定
auto_release_ms = 0       # 默认手动松开；可改为 300 等毫秒数
```

例如，`auto_release_ms = 300` 表示：长按点击键锁住鼠标后，配合 `Ctrl` 拖动，停住约 0.3 秒就会自动松开鼠标，Ctrl 仍需自行松开。该修饰键需正常传给应用；用 `N`、`press` 或 `toggle` 锁住的按钮仍需手动松开。两项时间均可设为 `0–60000` 毫秒。

## 自定义快捷键

打开[配置与模拟器](/editor/)，点选按键并选择动作，就能尝试新键位。喜欢直接改文件，也可以将下面的行合并到已有的 `[normal.bindings]`：

```toml
[normal.bindings]
"primary+space" = "left_click"
"primary+g" = "grid"
q = "idle"
```

这样就能用 `Primary+Space` 点击、`Primary+G` 打开网格。保存后，在托盘或状态栏菜单选择 **Reload Configuration**。

::: warning 请在原有按键表上修改
自己新建一张 `[normal.bindings]` 表会替换整张默认按键表，请保留仍要使用的移动、点击和退出键。建议从[完整默认配置](/generated/keysteer.default.toml)开始修改。
:::

在 Grid 或 UI Hint 中，同名按键优先用于选择标签；按住 `Primary` 可以临时使用 Normal 的鼠标操作。

未绑定的按键默认照常交给应用，也能与 AHK、Quicker 等工具配合。例如 `H` 移动鼠标时，未配置的 `Alt+H` 仍交给应用。若希望其他按键也不传给应用，可将 `[normal]` 的 `passthrough_unbound_keys` 设为 `false`；默认是 `true`。

## 临时输入文本

在 Normal 中需要打字时：

1. 按 `\` 开始输入。
2. 照常打字，`Ctrl/Cmd+A/C/V` 等应用快捷键也能使用。
3. 按 `Enter`、`\` 或 `Esc` 回到 Normal。

::: info Enter 默认只用于返回
它不会给应用换行或提交。需要换行、输入法选词或先提交再返回时，可按下面的示例修改 `[text_input.bindings]`。输入期间长按 `Q` 不会打开快速切换面板。
:::

::: code-group
```toml [默认：Enter 返回]
[text_input.bindings]
'enter \ esc' = "normal"
```

```toml [Enter 留给应用]
[text_input.bindings]
'\ esc' = "normal"
```

```toml [Enter 提交后返回]
[text_input.bindings]
'\ esc' = "normal"
enter = ["send enter", "normal"]
```
:::

如果想在打字时临时移动鼠标，可用右 Ctrl：按住它，再用 `H/J/K/L` 移动；松开后继续打字。

```toml
[text_input]
temporary_mode = "normal"
temporary_mode_keys = ["right_ctrl"]
```

临时操作结束时，持续移动和锁键也会结束。请选不会占用常用编辑快捷键的按键。若设置 `inherits = ["normal"]`，HJKL 等字母会执行鼠标动作，影响正常打字。

### 用主键区编辑文字（可选） {#可选主键区编辑映射}

想少挪动手指，可以自己启用下面的组合键。默认不启用，以免占用应用原有快捷键。

| 组合键 | 作用 |
| --- | --- |
| `Primary+H/J/K/L` | 左 / 下 / 上 / 右箭头 |
| `Primary+U/I/O` | Backspace / Delete / Insert |
| `Primary+T/Y` | Home / End |

从完整示例中挑选需要的行，加入已有 `[text_input.bindings]`。Ctrl、Shift 组合可用于选择文字；如果 Primary 已是 Ctrl，就跳过 Ctrl+Primary 的组合，避免重复。自己配置的完整组合键会优先于临时鼠标操作，长按仍按应用原有方式重复。

::: details 完整编辑键示例（按需复制）

```toml
# 按需选择编辑键；不要使用与 Primary 重复的修饰键。
# 自定义按键表会替换默认表，请保留返回键。
[text_input.bindings]
'enter \ esc' = "normal"
"primary+h" = "arrow_left"
"primary+j" = "arrow_down"
"primary+k" = "arrow_up"
"primary+l" = "arrow_right"
"primary+u" = "backspace"
"primary+i" = "delete"
"primary+o" = "insert"
"primary+t" = "home"
"primary+y" = "end"
"shift+primary+h" = "send shift+arrow_left"
"shift+primary+j" = "send shift+arrow_down"
"shift+primary+k" = "send shift+arrow_up"
"shift+primary+l" = "send shift+arrow_right"
"shift+primary+u" = "send shift+backspace"
"shift+primary+i" = "send shift+delete"
"shift+primary+o" = "send shift+insert"
"shift+primary+t" = "send shift+home"
"shift+primary+y" = "send shift+end"
"ctrl+primary+h" = "send ctrl+arrow_left"
"ctrl+primary+j" = "send ctrl+arrow_down"
"ctrl+primary+k" = "send ctrl+arrow_up"
"ctrl+primary+l" = "send ctrl+arrow_right"
"ctrl+primary+u" = "send ctrl+backspace"
"ctrl+primary+i" = "send ctrl+delete"
"ctrl+primary+o" = "send ctrl+insert"
"ctrl+primary+t" = "send ctrl+home"
"ctrl+primary+y" = "send ctrl+end"
"ctrl+shift+primary+h" = "send ctrl+shift+arrow_left"
"ctrl+shift+primary+j" = "send ctrl+shift+arrow_down"
"ctrl+shift+primary+k" = "send ctrl+shift+arrow_up"
"ctrl+shift+primary+l" = "send ctrl+shift+arrow_right"
"ctrl+shift+primary+u" = "send ctrl+shift+backspace"
"ctrl+shift+primary+i" = "send ctrl+shift+delete"
"ctrl+shift+primary+o" = "send ctrl+shift+insert"
"ctrl+shift+primary+t" = "send ctrl+shift+home"
"ctrl+shift+primary+y" = "send ctrl+shift+end"
```

:::

## 模式提示的样式与位置 {#模式标识符样式与位置}

鼠标旁的“Normal”文字会提醒你当前在哪个模式。可以改名字、颜色、字号和位置，也可以隐藏。

在[配置与模拟器](/editor/)中打开 **全局设置 → 模式标识符**，即可边改边看。拖动橙色锚点可调整位置，用方向键微调；按住 Shift 每次移动 10。

直接改 TOML 时，把设置合并到已有的同名表中：

```toml
[mode_indicator.modes.normal]
enabled = true   # false 表示隐藏
text = "Normal" # 可改成自己喜欢的名字

[mode_indicator.ui]
indicator_offset = [-12, 18] # 向左 12、向下 18
font_size = 13
background_color = "#0A1338FF"
text_color = "#E8EEFFFF"
border_radius = 6
padding_x = 8
padding_y = 4

[mode_indicator.modes.text_input]
enabled = false # 文本输入时默认隐藏；true 表示显示
```

`indicator_offset = [X, Y]` 定位提示框的右上角：X 正数向右、负数向左；Y 正数向下、负数向上。靠近屏幕边缘时，提示会自动挪回可见区域。两项可设为 `-32768–32767` 的整数。

全局样式会影响所有模式。只想改 Normal，就写在 `[mode_indicator.modes.normal.ui]`；删除单项设置或在网页中重置，即可恢复全局样式。Text Input 也可在自己的 `.ui` 表中单独设置。

旧配置中的 `position`、`indicator_x_offset`、`indicator_y_offset` 已停用，请改用 `indicator_offset = [X, Y]`。
