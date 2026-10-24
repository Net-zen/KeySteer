# Window：移动与控制窗口

<script setup>
import ModeVideo from '../.vitepress/components/ModeVideo'
</script>

移动、缩放、居中、切换窗口状态，并控制应用和系统声音。

<ModeVideo file="window.mp4" title="Window：移动与控制窗口" description="移动、缩放、居中、切换窗口状态，并控制应用和系统声音。" />

## 操作步骤与默认按键

进入时会激活并锁定鼠标下的窗口。之后鼠标移开，也不会自动换目标。屏幕上的数字表示窗口编号，输入编号即可激活目标并把鼠标移到它的中心。

| 按键 | Window 中的操作 |
| --- | --- |
| `H / J / K / L` | 向左 / 下 / 上 / 右移动；按住可连续调整 |
| `S` | 在移动与中心缩放之间切换 |
| `C` | 居中 |
| `D` | 移到下一块显示器 |
| `F` / `Shift+F` | 最大化／还原；最小化／还原 |
| 数字 | 按编号选择窗口 |
| `Tab / Shift+Tab` | 下一个 / 上一个窗口；在标签组内优先循环成员 |
| `X` | 请求关闭当前窗口；应用可能显示保存确认 |
| `Z / Shift+Z` | 撤销 / 重做窗口调整 |
| `Shift+C` | 恢复本次窗口会话初始的位置、尺寸及窗口状态 |

默认显示全部屏幕上未最小化的窗口。关闭窗口不会重排其他编号；重新进入时会清理空缺。若有 1 和 12 这样的编号，输入 1 后会短暂等待后续数字（默认 250 ms）。

可用 `screens = "current"` 只显示当前屏幕，更多设置见 [窗口配置](/reference/configuration#window-配置)。

::: tip 调窗口时也能操作鼠标
按住 `Primary`，用 Normal 的移动、滚动和点击键；松开后继续调整原窗口。Window 各模式分别改键，修改 Normal 不会同步修改这里。
:::

## 用网格快速移动

不想一路移动窗口？按 `G` 打开 Grid。窗口自身的 `Primary+F` 绑定为 Recursive Grid；默认临时层会优先使用 Normal 的同名组合键进入 UI Hint。要保留窗口绑定，将 `primary+f` 加入 `temporary_mode_passthrough_keys`。选择网格时，窗口中心会跟着定位点移动，边缘限制在屏幕可用区内。

回退、重选、切屏沿用网格模式的操作。完成定位后回到 Window；配置为 `keep` 时可继续选格。一轮网格定位可以用一次 `Z` 撤销。

::: info 网格入口单独改键
这两个入口在 `[window.bindings]` 中配置；修改 Normal 不会改变它们。裸 `F` 仍是最大化／还原，已有自定义 Window 绑定表需自行补入网格入口。
:::

## 音频控制

以下默认组合键在 Window、Quick 和 Editor 中可用。按住 `V` 再按对应键；系统操作额外按住 `Shift`。

| 操作范围 | 降低 / 提高音量 | 静音切换 | 上一个 / 下一个输出设备 |
| --- | --- | --- | --- |
| 当前目标窗口所属应用 | `V+J / V+K` | `V+M` | `V+H / V+L` |
| 系统 | `Shift+V+J / Shift+V+K` | `Shift+V+M` | `Shift+V+H / Shift+V+L` |

音量每次变化 1%；提高音量不会自动解除应用静音。同一应用的多个窗口可能共享音频会话。

- **Windows**：应用音量需要可用音频会话；没有会话时先让应用播放声音。切换设备后是否立即迁移当前声音取决于应用。
- **macOS**：系统音频支持 macOS 14+；应用独立音频需要 macOS 14.2+ 和系统音频录制权限，本地处理会增加音频延迟。某些输出设备不支持软件音量调节。

[返回窗口管理总览](/window-management/) · [完整配置参考](/reference/configuration)

## 选择多个窗口

1. 轻按 `Ctrl`，开始多选；当前窗口会先被选中。
2. 输入 `123` 选窗口 1、2、3；多位编号用空格隔开，如 `123 12 23`。同一编号再输一次可取消选择。
3. 按 `Ctrl` 或 `Enter` 确认，也可以直接按移动、缩放键一起调整。`E` 只排列这些窗口，`T` 将它们组成标签组。

选中的窗口会显示边框和 `✓`。输错可用退格修改，`Esc` 撤销这次输入并回到最初的窗口。

::: tip 清空选择
`Ctrl+X` 清空多选；进入 Quick、Restore 或退出时也会清空，并先回到最初的窗口。macOS 的 `Ctrl` 是 Control。
:::

按需修改以下按键，合并到已有表中：

```toml
[window.bindings]
ctrl = "window_multi_select"
"ctrl+x" = "window_multi_clear"

[window.multi_select.bindings]
"ctrl enter" = "window_multi_confirm"
"primary+h" = "arrow_left"
"primary+l" = "arrow_right"
```
