# 快速上手

<script setup>
import ModeVideo from '../.vitepress/components/ModeVideo'
import KeyLayout from '../.vitepress/components/KeyLayout'
</script>

让鼠标先休息一分钟，双手留在键盘上试试。KeySteer 可以用键盘移动鼠标、点击和整理窗口；先完成一次点击，其他功能用到时再学。

::: tip 先体验，不用背键位
这页先带你完成一次点击，再试着挪动窗口。完整键位图和配置放在后面，用到时再查。

无需先创建配置文件。平时 KeySteer 在托盘或状态栏待机，不影响正常打字。
:::

## 第一次使用

1. 启动 KeySteer。
2. 按进入键：

   | Windows | macOS |
   | --- | --- |
   | `左 Alt + E` | `Command + E` |

3. 按住下面四个键移动鼠标：

   ```text
          K  上
   H  左  J  下  L  右
   ```

4. 按 `;` 左键点击。
5. 按 `Esc` 返回待机，键盘恢复正常输入。

看到鼠标移动并完成一次点击，就已经学会 KeySteer 最常用的操作了。

::: warning macOS 第一次没有反应？
需要先授予控制权限：macOS 27 中名为“设备控制和数据访问”，旧版为“辅助功能”。参见 [macOS 安装与授权](/guide/macos)。
:::

文档里的 `Primary` 默认指 Windows **左 Alt**、macOS **Command**。字母大小写仅便于阅读，写出 `Shift+` 时才需按 Shift。

## 随时查看可用按键

::: tip 这是开卷操作
按 `?`（Shift+/）查看当前模式可用的按键，再按一次收起。记不住时，让提示面板带路就好。
:::

使用自定义绑定表时，保留 `"?" = "key_help"` 即可使用此快捷键。完整键位图和速查表放在本页后面，需要时查阅。

<ModeVideo file="help.mp4" title="查看当前快捷键提示" description="先预览提示面板，再给 key_help 配置喜欢的快捷键。" />

## 第一次用键盘调整窗口

<ModeVideo file="window.mp4" title="第一次用 Window 操控窗口" description="跟着视频练习移动、缩放、居中和声音控制。" />

1. 把鼠标放到一个普通窗口上。
2. 按 `Alt+W`（macOS 为 Option+W），然后松开入口键。
3. 用 `H/J/K/L` 移动窗口；按 `S` 切换中心缩放，再用相同方向键调整。
4. 按 `Z` 撤销一次调整，或按 `Q` 结束并保留结果。

想分屏？在 Window 中按 `A`，再按 `H` 放到左半屏。想整理多个窗口？按 `E` 立即自动排列，`Z` 可撤销。`T` 把窗口收成标签组，`R` 打开已保存预设。

Quick、Editor、Tabs、Restore 的 `Q` 返回 Window，再按一次 `Q` 回到待机。按住 `Primary` 可临时使用 Normal 的鼠标操作。

跟着 [Window 操作指南](/window-management/) 练习并排布局、标签分组、保存工作区和音频控制，并按当前模式查阅键位。

::: tip 放心试，Z 能撤销
`Z` 撤销一次调整，`Shift+Z` 重做。按 `Q` 退出会保留当前布局。
:::

## 接着试试窗口管理

| Window 中按 | 会发生什么 | 继续学习 |
| --- | --- | --- |
| `A` | 方向键安排窗口位置与分屏比例。 | [Quick：快捷分屏](/window-management/quick) |
| `E` | 自动平铺，再交换窗口、切分与调整区域。 | [Editor：平铺与区域编辑](/window-management/editor) |
| `T` | 自动整理同应用的兼容窗口，也可自由组合。 | [Tabs：窗口标签分组](/window-management/tabs) |
| `R` | 保存的是空间和分组安排，恢复时用当前需要的窗口填入。 | [Restore：恢复布局与标签模板](/window-management/restore) |

在 Editor 或 Tabs 中按 `Ctrl+S` → 填写备注或留空 → `Enter` 保存；留空时自动生成默认名称。

## 一张图记住工作方式

```mermaid
---
config:
    layout: dagre
    look: classic
    flowchart:
        curve: basis
        rankSpacing: 40
        minNodeWidth: 88
        padding: 12
---
flowchart LR
    idle("待机<br/>正常打字"):::mode-step
    normal("Normal<br/>移动、点击、滚动"):::mode-primary
    target("快速定位<br/>Grid<br/>Recursive Grid<br/>UI Hint"):::mode-step

    idle -->|"Primary+E"| normal
    normal -->|"需要时再进入"| target
    target -->|"Esc"| normal
    normal -->|"Esc / q"| idle
    class idle diagram-blue
    class normal diagram-violet
    class target diagram-green
```

平时只需在“待机”和 “Normal” 之间切换。三种定位模式是可选的，不需要一开始全部记住。



```mermaid
---
config:
    layout: dagre
    look: classic
    flowchart:
        curve: basis
        rankSpacing: 40
        minNodeWidth: 88
        padding: 12
---
flowchart LR
    idle("待机<br/>正常打字"):::mode-step
    window("Window<br/>移动、大小、音量"):::mode-primary
    arrange("窗口整理<br/>Quick<br/>Editor<br/>Tabs"):::mode-step

    idle -->|"Alt+W"| window
    window -->|"需要时再进入"| arrange
    arrange -->|"Q"| window
    window -->|"Q"| idle
    class idle diagram-blue
    class window diagram-violet
    class arrange diagram-amber
```

Window 是窗口管理的基础入口，Quick、Editor 和 Tabs 是按需选用的整理方式。

## 鼠标移动太远时

| 你想做什么 | 按键 | 模式 |
| --- | --- | --- |
| 快速到达屏幕的大致区域 | `g` | [Grid](/modes/grid) |
| 精确定位很小的目标 | `Primary+G` | [Recursive Grid](/modes/recursive-grid) |
| 用方向键逐次二分定位 | `Primary+F` | [Bisect](/modes/bisect) |
| 直接选择按钮、链接或输入框 | `f` | [UI Hint](/modes/ui-hint) |

进入定位模式后按 `Esc` 返回 Normal；再按一次 `Esc` 返回待机。

## 按键不顺手？

打开 [配置与模拟器](/editor/) 直接查看键盘、修改绑定和颜色，然后下载自己的 TOML。KeySteer 不依赖配置文件；建议在 [默认文件](/generated/keysteer.default.toml) 的基础之上修改。

`Primary` 是跨平台名称：发布默认值在 Windows 上是左 `Alt`，在 macOS 上是 `Command`。高级用户可以通过 `[key_aliases]` 改成其他实体键。

::: info 常用入口都在状态菜单
右键 Windows 托盘图标，或点击 macOS 顶部状态图标，可以暂停、重载配置、设置开机启动、检查更新或退出。选择 **Configuration & Simulator...** 可把当前配置带入网页编辑器。
:::

配置和日志在 Windows 便携版的程序旁边；macOS `.app` 使用 `~/Library/Application Support/KeySteer/`。

遇到问题时，先运行环境诊断；改过配置后，可以单独检查文件：

```bash
keysteer --doctor                       # 检查环境和权限
keysteer --check -c keysteer.user.toml   # 检查配置
```

完整参数和排错方法见[配置文件](/reference/configuration)，动作写法见[模式与动作](/reference/modes-and-actions)。

## 在浏览器里图形化改键

从托盘右键菜单打开 **Configuration & Simulator...**，跳转浏览器（注意网络连接）。模拟器供编辑和预览，实际行为以程序为准。

<ModeVideo file="config.mp4" title="修改 W、A 按键绑定" description="点击完整键盘中的按键，再选择动作。" />

## 常用操作

<KeyLayout
  layout="q w e r t y u i o p/Caps a s d f g h j k l ; '/Shift z x c v b n m , . Slash RShift/Ctrl Primary Alt Space"
  move="h j k l"
  click="; ' RShift"
  speed="Caps Shift v b"
  scroll="m ,"
  state="n"
  navigation="t y u i"
  mode="e f g q Primary"
  label="常用键位速记"
  hint="先认颜色，再按需记住其他键"
/>

| 你想做什么 | 按键 | 小提示 |
| --- | --- | --- |
| 移动鼠标 | `H / J / K / L` | 左 / 下 / 上 / 右 |
| 精确 / 慢速移动 | `Caps Lock` / `左 Shift` | 按住后移动，也影响滚动 |
| 快速移动 | `V` 或 `B` | 按住后移动，也影响滚动 |
| 向下 / 向上滚动 | `M` / `,` | 在主键区滚动 |
| 左键 / 右键 / 中键点击 | `;` / `'` / `右 Shift` | 点击键在右手附近 |
| 拖拽 | `N` + 点击键 | 单独点 `N` 松开 |
| Home / End / Page Up / Page Down | `T / Y / I / U` | 直接发送给当前应用 |
| 打开定位模式 | `G` / `F` / `Primary+G` / `Primary+F` | Grid / UI Hint / Recursive Grid / Bisect |
| 将鼠标切到下一块显示器 | `Primary+S` | 多屏时使用 |
| 将鼠标下的窗口移到下一块显示器 | `Primary+D` | 保持鼠标在窗口内的相对位置 |
| 返回待机 | `Q` 或 `Esc` | 恢复正常打字 |

更多拖拽、调速和改键方法见 [Normal 普通模式](/modes/normal)。
