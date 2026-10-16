# 模式与动作参考

改绑定时，在这页查动作名称和参数。第一次改键可先看[最小配置](/reference/configuration#最小配置)。

配置中的每个绑定都写成：

```toml
"按键" = "动作"
```

也可以写成动作数组：

```toml
"按键" = ["动作 1", "动作 2", "动作 3"]
```

数组按书写顺序执行；空数组不合法。数组不会阻塞输入线程，`wait` 只会暂停当前序列。

## 模式名

| 值 | 作用 |
| --- | --- |
| `idle` | 待机，只监听 `[hotkeys]`，不拦截普通输入。 |
| `normal` | 直接移动、滚动、点击，并进入定位模式。 |
| `grid` | 全屏坐标网格。 |
| `recursive_grid` | 在当前区域中逐层细分。 |
| `ui_hint` | 给可交互元素显示标签。 |
| `window` | 锁定鼠标下的窗口，移动、中心缩放、布局和平铺。 |
| `window_quick` | Quick 比例布局。 |
| `window_editor` | 自动布局与分区编辑。 |
| `window_restore` | 恢复布局或标签模板。 |
| `window_tab` | 将窗口组成可切换的标签组。 |
| `plugin:<id>` | 插件模式，例如 `plugin:screen-selector`。 |

```toml
[normal.bindings]
g = "grid"
f = "recursive_grid"
"primary+f" = "ui_hint"
"primary+s" = "screen next"

# 可选：向当前应用发送方向键
# "primary+h" = "left"
# "primary+j" = "down"
# "primary+k" = "up"
# "primary+l" = "right"
```

## Window 模式

想先练习操作？看 [Window 指南](/modes/window)；这里用于查动作和可配置行为。

| 模式 | 默认入口 | 默认 Q 目标 |
| --- | --- | --- |
| `window` | 全局 `Alt+W`（macOS 为 Option+W） | `idle` |
| `window_quick` | Window 的 `A` | `window` |
| `window_editor` | Window 的 `E` | `window` |
| `window_restore` | Window 的 `R` | `window` |
| `window_tab` | Window 的 `T` | `window` |

每个模式独立配置按键、继承、应用例外和样式。Q 的去向由绑定决定，与进入路径无关；例如直接进入 Editor，Q 仍返回 Window。再次触发当前模式的入口会回到 Idle。

### 选择窗口和操作范围

```toml
[window]
screens = "all"           # "current" 当前屏幕；"all" 全部屏幕（默认）
include_minimized = false # true 时也包含最小化窗口
```

| 配置 | Window 默认 | 其余四个模式默认 |
| --- | --- | --- |
| `screens` | `"all"`，全部屏幕 | `"current"`，当前屏幕 |
| `include_minimized` | false | false |

开启 `include_minimized` 可选择最小化窗口。自动排列和分组仍按屏幕分别进行，不会自动跨屏搬动。会话内编号保持稳定；重新进入时按当前候选重新编号。卡片会避让其他编号和帮助面板。

Alt+W 默认选中鼠标下的窗口，随后不会因鼠标移开而换目标。输入完整编号会选中窗口并把鼠标移到中心；可能组成多位编号时才等待，默认 250 毫秒。

| 默认键 | 动作 | 用途 |
| --- | --- | --- |
| `H/J/K/L` | `window_left/down/up/right` | 移动；S 切换为中心缩放 |
| `S` | `window_size` | 切换移动／缩放 |
| `D` | `window_screen_next` | 移到下一屏 |
| `F` / `Shift+F` | `window_maximize` / `window_minimize` | 最大化／还原；最小化／还原 |
| `C` | `window_center` | 居中 |
| `Tab` / `Shift+Tab` | `window_select` / `window_select_previous` | 下一个／上一个；组内优先循环标签 |
| `X` | `window_close` | 请求关闭；应用可能提示保存 |
| `Ctrl` / `Ctrl+X` | `window_multi_select` / `window_multi_clear` | 开始多选／清空多选 |

多选时按 Ctrl 或 Enter 确认，也可直接执行移动、缩放。`E` 只排列所选窗口，`T` 将它们合成一组。完整步骤见 [选择多个窗口](/window-management/window#选择多个窗口)。X 在标签组中只关闭当前成员，剩一个成员时自动解散；Editor 的 X 仍是删除区域。

### Quick、Editor 和预设

| 模式 | 主要操作 |
| --- | --- |
| Quick | H/J/K/L 调整比例：同方向缩小，反方向扩大；横纵轴独立。首次取最接近半屏的比例，满屏自动加入；Tab 换窗并重置比例 |
| Editor | 进入即自动排列；方向键选区、Shift＋方向切分、Ctrl+H/L 调宽、Ctrl+K/J 调高；相邻区域一起调整 |
| Editor 选窗 | 连输两个窗口编号交换；反引号后输区域编号可选区或移入空区域。新区域复用空闲编号，已有区域不重排 |
| 保存 | Editor／Tabs 按 Ctrl+S，输入可选备注后 Enter；最多 80 字符，留空为 Layout N／Tabs N |
| Restore | 输入编号恢复；PageUp/PageDown 翻页。布局成功后默认进 Editor，失败留在列表显示原因 |
| 删除 | Restore 按 X 切换删除状态，选编号、核对名称、Enter 确认；再次 X 回到恢复 |

修改即时生效，退出不会自动撤销。布局只保存区域、窗口数量和备注；恢复时按当前窗口最近活动顺序填入，多余窗口不动，不足则留空，不会启动应用或重开文档。

布局和标签模板共用数据目录中的 `workspace.ksw`。可与[模拟器](/simulator)互导；下载后替换数据目录的同名文件，再进入 Restore 读取。删除预设不影响现有窗口或标签组；保存失败时保留原文件，外部修改过的记录会要求重新选择。

### 撤销、重做与恢复原位

| 默认键 | 动作 | 效果 |
| --- | --- | --- |
| `Z` | `window_undo` | 撤销一步；连续移动／缩放按一组处理 |
| `Shift+Z` | `window_redo` | 重做；新的调整清空重做记录 |
| `Shift+C` | `window_reset_initial` | 恢复本次会话调整过的窗口的位置、尺寸和状态 |

这三项在 Window、Quick、Editor 中默认可用。窗口模式之间切换保留会话起点；退出到 Idle 等组外模式后重新进入，才建立新起点。

普通调整保留最近 32 组历史。Editor 内逐步撤销，离开后整轮算一组；原始状态不受 32 组限制，恢复原位本身也可撤销。撤销不会重开已关闭窗口或恢复应用内容。

### 网格定位入口

Window 的网格入口独立于 Normal：

```toml
[window.bindings]
g = "grid"
"primary+f" = "recursive_grid"
```

G 进入 Grid，Primary+F 进入 Recursive Grid，裸 F 仍最大化／还原。可改键或设 `none` 禁用；已有自定义表需补入对应行。其他窗口子模式没有这两个默认入口，按住 Primary 时还会按临时层规则处理。

### T：持续存在的标签组合

从 Window 按 T：有多选就把所选窗口合成一组（已有组的成员会带入整组）；没有多选时，按屏幕分别组合同应用的兼容、未分组窗口。已有组保留，Z 可撤销这次整理。

手动分组时，先输一个窗口编号，再输另一个就立即成组；继续输编号可追加成员，T 结束这一组并开始下一组。空格用于结束编号，`~` 用于选择整组。

| 输入 | 结果 |
| --- | --- |
| `12t34t` | 窗口 1、2 一组，3、4 一组；仅适用于编号没有多位歧义时 |
| `123t` | 窗口 1、2、3 一组 |
| `~1` | 选择组 1 |
| `~1 4t` | 窗口 4 加入组 1 |
| `~1 ~2t` | 整组 2 并入组 1，保留组 1 编号 |
| `1 空格 2 t` / `12 空格 t` | 明确选择窗口 1、2／窗口 12 |

窗口编号选一个成员，组编号选整组。已有成员再次选中只切换标签；无效编号显示原因并保留当前目标。新组使用最小空闲编号；全部解散后从 ~1 开始。T 会先完成有效的待输入编号，不另加撤销记录。

| 默认键 | 动作／效果 |
| --- | --- |
| `T` | `window_tab_end`：结束当前组；不足两个窗口时清除选择 |
| `~` / 空格 | `window_tab_group` / `window_number_end`：组号前缀／编号分隔符 |
| `D` / `X` | `window_tab_remove` / `window_tab_dissolve`：移出当前成员／解散组 |
| `Tab` / `Shift+Tab`，或 `J` / `K` | 下一个／上一个标签 |
| `H` / `L` | 向左／右调整标签顺序 |
| `Z` / `Shift+Z` | 撤销／重做分组、移出、解散和排序 |
| `Ctrl+S` | `window_save_layout`：保存模板与备注 |
| `Q` / `Esc` | 返回 Window／退出到 Idle，保留已完成分组 |

这些按键在 `[window_tab.bindings]` 中修改。退出后应用的 Tab、数字照常使用，标签栏仍可用鼠标切换。Windows 还可拖动标签排序、跨组转移，或拖动 `~组编号` 合并整组；插入线表示落点，移出后松开或右键取消，滚轮可浏览长标签栏。

每组只显示活动成员，Quick／Editor 把整组当作一个目标。切换时新成员对齐组的位置；最小化活动成员会收起整组。关闭标签栏只解散分组，关闭应用只移除该成员；解散或正常退出 KeySteer 会恢复本程序收起的窗口。

::: info 保存模板，跨次复用
标签组保留到解散或退出 KeySteer。模板保存成员数量、顺序、活动位置、区域和备注，不保存应用身份。恢复时依次选窗，选满后才应用；选满前可用 Q/Esc 取消。
:::

支持普通可缩放窗口，原生全屏或不兼容窗口会提示原因。Windows 隐藏非活动成员；macOS 使用逐窗口最小化／恢复，可能看到系统动画。标签栏占用组顶部空间，不遮挡应用底部。

### 当前窗口所属应用音量

Window、Quick、Editor 中，按住 V 后用 J/K 降低／提高应用音量，每步 1%；V+M 切换静音。方向键按住可连续调整，松开 V 停止。提高音量不会自动取消静音；同应用的多个窗口可能一起变化。

已有自定义绑定表时，将下面三行加入对应模式：

```toml
"v+j" = "window_volume_down"
"v+k" = "window_volume_up"
"v+m" = "window_volume_mute"
```

### 应用输出与系统音频快捷键

| 默认键 | 动作 |
| --- | --- |
| `V+H` / `V+L` | `window_audio_previous` / `window_audio_next`：应用上一个／下一个输出设备 |
| `Shift+V+J` / `Shift+V+K` | `window_system_volume_down` / `window_system_volume_up`：系统音量 |
| `Shift+V+M` | `window_system_volume_mute`：系统静音切换 |
| `Shift+V+H` / `Shift+V+L` | `window_system_audio_previous` / `window_system_audio_next`：系统输出设备 |

设备按名称排序，每按一次切换一个；面板显示目标设备名。应用输出不改变系统默认设备，有些应用需重新播放才采用新设备。静音键按住不会反复切换。

::: info 音频可用性
Windows 应用需要先有音频会话，没有时可先播放声音。macOS 系统音频支持 14+；应用独立音频需 14.2+、打包的 KeySteer.app 和系统音频录制权限。
:::

macOS 应用音频在内存中处理，不保存或上传录音，但会增加播放延迟；设置在退出模式后保留，退出 KeySteer 时恢复原始路径。设备不支持软件音量时会显示原因。技术说明见 [Apple Core Audio Tap](https://developer.apple.com/documentation/CoreAudio/capturing-system-audio-with-core-audio-taps)。

## 移动、滚动和速度

| 动作 | 说明 |
| --- | --- |
| `move_left`、`move_down`、`move_up`、`move_right` | 按住时连续移动；短按也会移动一小段。 |
| `scroll_left`、`scroll_right`、`scroll_up`、`scroll_down` | 按 `[scroll].scroll_step` 滚动。 |
| `scroll_half_*` | 按 `scroll_step_half` 滚动。 |
| `scroll_full_*` | 按 `scroll_step_full` 滚动。 |
| `precision`、`slow`、`fast` | 按住时改变移动速度。 |
| `precision_toggle`、`slow_toggle`、`fast_toggle` | 按一下锁存速度，再按同一键关闭；状态显示在模式指示器下方。 |
| `follow` | 切换 Grid/Recursive Grid 的鼠标跟随。 |

`wheel_*` 是 `scroll_*` 的兼容别名。速度动作通常和方向键一起使用：

```toml
[normal.bindings]
h = "move_left"
"v b" = "fast"
```

`"v b"` 表示两个独立按键绑定到同一动作，不是按键序列；组合键使用 `+`。

## 鼠标按钮与拖拽

| 动作 | 用途 |
| --- | --- |
| `left_click` / `right_click` / `middle_click` / `mouse_x1` / `mouse_x2` | 点击对应鼠标键；默认支持长按后保持按下 |
| `double_click` | 短按双击；长按只保持左键 |
| `left_press` / `right_press` | 保持左／右键按下 |
| `left_release` / `right_release` | 松开左／右键 |
| `toggle_left` / `toggle_right` | 切换左／右键的按住状态 |
| `toggle`（无参数） | 与伙伴键配合，锁定其在 Normal 中对应的键或鼠标按钮 |
| `press <目标...>` / `release <目标...>` / `toggle <目标...>` | 按住／释放／切换一个或多个指定目标 |

目标可写键名或 `mouse_left`、`mouse_right`、`mouse_middle`、`mouse_x1`、`mouse_x2`：

```toml
[normal.bindings]
n = "toggle"
x = ["press shift", "left_click", "release shift"]
```

无参数 toggle 默认是 N：N 与 `;` 配合保持左键，先按哪个都可以，也能锁定多个伙伴。单独短按 N、返回 Normal 或进入 Idle 会全部释放；单独长按 N 会锁定 N 自身。

`long_press_toggle_ms` 默认 500 毫秒。启用时，按下点击键就按下鼠标，松开就释放；超过阈值后保持，设 0 则改为按下时立即完成点击。双击键短按释放时完成双击。自动松开条件和操作步骤见 [拖拽与长按](/modes/normal#拖拽与长按)。

## 发送按键

裸键名会发送给当前聚焦应用；组合键可以直接使用 `+`，也可以使用更明确的 `send`：

```toml
[normal.bindings]
t = "home"
"primary+shift+s" = "send primary+shift+s"
```

`send` 后面必须是一个合法键或组合键。它不会切换 KeySteer 的模式，只把合成按键注入当前应用。

## 执行外部命令：`exec`

用 `exec` 启动脚本、应用或其他桌面工具。下面是 macOS 示例：

```toml
[normal.bindings]
"primary+shift+t" = "exec open -a Terminal"
"primary+shift+b" = ["exec say build-started", "wait 500", "exec open ."]
```

语法：

```text
exec <program> [arg1] [arg2] ...
```

第一个词是程序名，后续词是独立参数。程序在后台启动，KeySteer 不等待完成，也不显示命令输出；启动失败会写入日志。

::: info 需要 shell 语法时
`exec` 直接执行程序，不展开 `~`、环境变量、管道、重定向或 `&&`。把复杂逻辑放进脚本，或显式调用 shell。
:::

```toml
# 直接执行不含空格的脚本或程序
x = "exec /usr/local/bin/keysteer-script"

# Windows：参数按空格拆分
y = "exec cmd /C start notepad"
```

参数按空格切分，不支持引号转义。路径含空格或参数复杂时，使用无空格路径的包装脚本。

## 插件动词与参数

插件可以在 Manifest 中注册动词。带参数时可以直接写：

```toml
[normal.bindings]
"primary+s" = "screen next"
"primary+1" = "screen 1"
"primary+shift+s" = "call screen"
```

- `screen next`：调用插件动词 `screen`，参数是 `next`。
- `screen 1`：调用同一个动词，参数是 `1`。
- `call screen`：显式调用无参数动词。

显式 `call` 适合无参数调用或避免和其他绑定语义混淆。未知的小写动词加参数会作为插件调用；拼写错误的内置动作会在加载时失败，而不是静默发送按键。

## 直接切换窗口

`window_activate_next` / `window_activate_previous` 直接切换窗口，不进入 Window，也不显示编号。

默认从鼠标下的窗口开始，没有则用激活窗口。切换到目标后，鼠标移到它的中心。列表包含全部未最小化的普通窗口；同一 Tabs 组按标签顺序相邻，同屏同应用也尽量相邻。走完一组会继续到其他应用，例如 A1 → A2 → B1 → B2。

没有默认快捷键，可自行添加：

```toml
[normal.bindings]
x = "window_activate_next"
c = "window_activate_previous"
```

## 相交窗口切换

`window_overlap_next` / `window_overlap_previous` 只在互相重叠的一组窗口中循环，不进入 Window。

例如 A 与 B 重叠、B 与 C 重叠，即使 A 不直接碰到 C，三者仍是一组；仅边缘接触不算。每次按键都按最新位置判断，最小化窗口不参与。

默认先用激活窗口，没有则用鼠标下窗口。Tabs 成员优先按标签顺序切换，仍只选择重叠组内的窗口。只有一个窗口时，仅把鼠标移到其中心；没有有效目标则不操作。

```toml
[normal.bindings]
x = "window_overlap_next"
c = "window_overlap_previous"
```

## 跨屏移动窗口

`move_window next` 移到下一屏，`previous`（或 `prev`）移到上一屏，`move_window 2` 移到编号 2 的屏幕；编号与 `screen` 插件一致。

默认 `Primary+D` 移动鼠标下窗口，鼠标跟随且尽量保持相对位置，前台焦点和当前模式不变。`Primary+S` 只切换鼠标所在屏幕。只有一块屏幕或没有可移动窗口时不操作。

| 情况 | 移动方式 |
| --- | --- |
| 屏幕尺寸相同 | 保持窗口大小和偏移 |
| 屏幕尺寸不同 | 按可用空间比例调整位置，尽量放入工作区 |
| Windows 最大化窗口 | 保持最大化，并迁移还原位置 |
| macOS 原生全屏窗口 | 退出全屏、跨屏、再恢复；会保留系统动画 |

鼠标会限制在目标屏幕内。应用自身可能限制移动、全屏切换或 DPI 下的尺寸。

## 其他动作

| 动作 | 参数和作用 |
| --- | --- |
| `move_mouse <x> <y>` | 移动到绝对桌面坐标；需要两个整数。 |
| `wait` 或 `wait 0` | 等待默认 `100ms`。 |
| `wait <max_ms>` | 在 `0` 到上限之间随机等待。 |
| `wait <min_ms> <max_ms>` | 在范围内随机等待；最大为 `86400000ms`。 |
| `finish` | 完成当前定位会话。 |
| `restart_mode` | 清空当前定位会话并重新开始。 |
| `rescan` | 重新扫描 UI Hint。 |
| `escape` | 离开当前模式并返回 Idle。 |
| `reload_config` | 从磁盘重新加载配置。 |
| `set_config <path> <TOML值>` | 修改点号路径并持久化，例如 `set_config pointer.max_speed 800`。 |
| `quit` | 退出程序。 |
| `none` | 禁用该绑定，常用于屏蔽继承的按键。 |

`set_config` 的值必须是合法 TOML 值；字符串要带引号，数组和表也可以直接传入。它只修改当前加载的配置文件；若本次是纯内置默认值启动，请先用 `--config` 指定一个文件：

```toml
[normal.bindings]
"primary+1" = "set_config pointer.max_speed 800"
"primary+2" = "set_config general.excluded_apps [\"com.example.App\"]"
```

修改会先解析和校验，成功后才写入配置；失败时保留最后一份有效配置。

## 绑定解析顺序

右值的解析顺序是：

1. `none` / `__disabled__`。
2. 显式动作：`call`、`send`、`exec`、`move_mouse`、`set_config`、`press`、`release`、`toggle`、`wait`。
3. 内置动作，如 `move_left`、`left_click`、`fast`、`finish`。
4. 带参数的插件动词。
5. `+` 组合键或已知裸键，作为发送给当前应用的按键。
6. 内置模式名或命名空间插件模式名。

完整默认示例见 [默认配置文件](/generated/keysteer.default.toml)。

## `key_help`

显示或隐藏当前模式的按键提示，保留选择和筛选状态。

Normal 默认绑定 `"?" = "key_help"`，定位模式可继承；自定义按键表请保留这一项。默认显示状态和外观见 [实时按键提示](/reference/configuration#实时按键提示)。

## UI Hint 扫描范围

```toml
[ui_hint]
scan_scope = "window" # window / active / screen
```

| `scan_scope` | 扫描哪里 |
| --- | --- |
| `window`（默认） | 鼠标下窗口；没有则用激活窗口，再没有则用鼠标所在屏幕 |
| `active` | 激活窗口优先，其次鼠标下窗口，最后鼠标所在屏幕 |
| `screen` | 鼠标所在整屏，不合并其他屏幕 |

这些规则适用于所有识别方式，两平台相同。默认选中鼠标窗口时会尝试激活它。选定范围后，空结果或识别失败不会改换范围；激活失败也不改范围。

切屏或更换目标会清除旧标签并重新识别；普通鼠标移动不会持续重扫。`Primary+R` 可手动刷新。

## 窗口目标来源

想操作激活窗口，还是鼠标下窗口？五个窗口模式可分别设置 `target`：

```toml
[window]
target = "active"
[window_quick]
target = "mouse"
[window_editor]
target = "active"
[window_restore]
target = "mouse"
[window_tab]
target = "active"
```

| 值 | 优先顺序 |
| --- | --- |
| `active` | 激活窗口 → 鼠标下窗口 |
| `mouse` | 鼠标下窗口 → 激活窗口 |
| 省略 | 新会话用鼠标下窗口；窗口子模式切换保留已选目标 |

显式设置时，每次进入该模式重新选窗。也可给某个动作加后缀，优先于模式的 `target`：

```toml
[normal.bindings]
"alt+j" = "window_overlap_next active"
"alt+k" = "window_overlap_next mouse"
"alt+n" = "window_activate_next active"
"alt+w" = "window mouse"
"alt+d" = "move_window next active"
[window.bindings]
"x" = "window_close active"
"shift+x" = "window_close mouse"
"h" = "window_left mouse"
```

模式内不带后缀的动作继续操作已选窗口。带后缀时，每次开始操作重新选窗，按住移动／缩放期间固定目标。首选窗口有效就不会改用另一个来源；如果操作被拒绝，也不会转而操作另一窗口。

| 可以加后缀 | 不接受后缀 |
| --- | --- |
| 窗口切换、模式入口、跨屏移动 | 撤销、重做、恢复原始状态 |
| 移动、缩放、状态、关闭、应用音频 | 预设保存、删除、确认、编号输入 |
| 布局方向、切分、比例、删除区域、标签成员操作 | 系统音频 |

布局只能选已有区域内的窗口；标签操作只影响所选窗口的组，不会合并组。相交切换若无同伴，只把鼠标移到首选窗口中心。两个来源都没有有效窗口时不操作。

## UIHint 搜索与信息复制

先看 [UI Hint 操作示例](/modes/ui-hint#搜索与结果预览)，这里列出搜索语法、复制和样式参数。

### 搜索与确认

按 `/` 开始搜索，首批标签未出现时也能输入。可用标签、OCR 文字、辅助功能名称或控件类型：如 `复制` / `fz`、`按钮` / `an` / `button`。中文支持简拼或粘贴，暂不支持系统输入法候选窗口。

| 操作 | 默认键／结果 |
| --- | --- |
| 预览下一个结果 | `Tab`；只切预览，不移动鼠标 |
| 确认预览 | `Enter`、`/`、`Primary+Q`；移动鼠标、清空查询，留在 UI Hint |
| 取消搜索 | `Esc`；不移动鼠标，恢复全部标签 |
| 只搜标签 | `@la` 或 `la@`；`@l` 按标签前缀缩小范围 |

单独 `@` 保留候选，不代表全选。输入普通标签只筛选，不会立即跳转；搜索期间标签代码保持不变。没有有效结果时，确认不会移动鼠标。

### 多选与结果顺序

空格分隔独立选择，例如 `@la @ka` 或 `fz button ab`。结果按输入顺序合并，重复目标保留第一次。连续空格不加空项，末尾空格显示下一项候选；修改任一项会更新结果。

完整标签可直接选中；文字／简拼需要唯一命中才计入已选点。多点初始显示平均中心 `0/N`；N 只计入已确定且去重的标签，未完成或有歧义的项不计入。

### 复制四栏信息

| 默认键（两平台相同） | 内容 |
| --- | --- |
| `Ctrl+1` | OCR 文字 |
| `Ctrl+2` | 辅助功能名称与控件类型 |
| `Ctrl+3` | 当前点坐标 |
| `Ctrl+4` | 当前点颜色 |

复制信息成功会确认当前点、移动鼠标并退出搜索，保留 UI Hint 标签。空字段或复制失败则留在搜索，剪贴板不变。显示时省略的长文字仍会完整复制。

`search_copy_keys` 必须包含四个互不冲突的组合键。使用 `primary` 时遵循平台别名；默认仍是 Ctrl，不是 macOS Command。

### 微调位置和取色

轻按 Ctrl 切换输入／调整状态，调整时用 Normal 的方向键和速度键。绿色输入框和右侧圆点表示正在调整；点击等操作会先确认点位，再执行原操作。

`Ctrl+Shift+4` 可直接进入调整，并循环 HEX → RGB → HSL；`Shift+4` 仍可输入 `$`。

```toml
[ui_hint.search_bindings]
ctrl = "point_toggle"
tab = "point_next" # 可换成其他按键或组合键
"ctrl+shift+4" = "color_next"
```

`point_toggle` 必须是单键轻触；`point_next`、`color_next` 支持组合键。自定义 `search_bindings` 替换整张表，空表禁用这些操作。

多点调整时，Tab 从平均中心切到第一个标签，再按选择顺序循环，显示 `1/N…N/N`。标签初始点与直接输入该标签时的跳转点相同，不受标签显示偏移影响。

`[ui_hint.search_point]` 支持 `marker_color`、`marker_radius`、`marker_width`；`color_formats` 设置不重复的格式顺序，第一项为默认，如 `["rgb", "hex", "hsl"]`。

颜色来自屏幕，排除 KeySteer 自身覆盖层；macOS 需要录屏权限。复制颜色会等待当前取样完成；失败时保留空位，不复制旧颜色。

### 多点信息如何展示

`field_modes` 四项依次对应 OCR、辅助信息、坐标、颜色：

```toml
[ui_hint.search_point]
field_modes = ["concat", "concat", "switch", "switch"] # 内置默认值，省略即可
```

| 值 | 展示与复制 |
| --- | --- |
| `concat` | 按选择顺序换行拼接各标签初始点的信息 |
| `switch` | 跟随当前检查点，Tab 或方向移动时更新 |

默认文字和辅助信息拼接，坐标和颜色跟随当前点。单点文字显示当前点命中的元素，空白处只有坐标和颜色。网页 **UI Hint → 行为** 可修改这些选项。

### 搜索框编辑键

`[ui_hint.search_edit_keys]` 使用“按键 = 动作”，如 `"enter / primary+q" = "accept"`、`"primary+v" = "paste"`、`"primary+c" = "copy"`、`"primary+a" = "select_all"`。

左侧空格分隔多个快捷键；修改某动作会替换该动作全部默认键，省略的动作保留默认。旧的“动作 = 按键”仍可读取，导出统一用新格式。复制输入选区只复制查询文字，不退出搜索。

### 面板与取色样式

下面示例设置复制键和颜色预览块。单点颜色标题旁显示色块，多点颜色拼接时不显示单个色块。

```toml
[ui_hint]
search_copy_keys = ["ctrl+1", "ctrl+2", "ctrl+3", "ctrl+4"]

# 可选覆盖，下面展示内置默认值（逻辑像素）
[ui_hint.search_point.color_preview]
enabled = true
width = 16         # 1–64
height = 16        # 1–64；与 width 相同即为正方形
x_offset = 4       # -200–200，相对标题末尾，正值向右
y_offset = 0       # -200–200，相对标题行中心，正值向下
border_width = 1   # 0–10，0 不画边框
```

点位调整状态可以单独改边框和背景：

```toml
[ui_hint.search_point]
input_border_color = { light = "#16856BFF", dark = "#68D9B1FF" }
# 可选覆盖默认绿色背景
# input_background_color = { light = "#E5F3ECF2", dark = "#16382FF2" }
```

默认使用浅色 `#E8F6F0FF`／深色 `#284D44FF` 绿色背景；未覆盖的边框沿用 `search_input_ui.border_color`。网页 **UI Hint → 外观** 可编辑、预览或重置。

搜索框和信息面板支持相同的字体、颜色、边框、圆角和内边距设置。默认搜索框宽 280、垂直内边距 6；下面演示自定义外观：

```toml
[ui_hint.search_input_ui]
position_mode = "screen" # 或 window
position = ["95%", "50%", "5%", "50%"] # 上、右、下、左
width = 420
font_family = "Microsoft YaHei UI"
font_size = 16
padding_x = 12
padding_y = 6
border_radius = 8
border_width = 1
background_color = "#0A1338F2"
text_color = "#E8EEFFFF"
border_color = "#6E82D6FF"

[ui_hint.search_info_ui]
position_mode = "search_input" # 也支持 screen / window
position = ["44%", "50%", "56%", "50%"] # 上方留少量间距
width = 520
font_size = 14
```

位置用参照对象和四边百分比，详见 [搜索面板定位](/reference/configuration#搜索面板定位)。网页可同时预览两个面板，默认样式无需重复写入配置。
