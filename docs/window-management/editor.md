# Editor：平铺与区域编辑

<script setup>
import ModeVideo from '../.vitepress/components/ModeVideo'
</script>

在 Window 按 E 立即自动平铺，再交换窗口、切分与调整区域。

<ModeVideo file="editor.mp4" title="Editor：平铺与区域编辑" description="在 Window 按 E 立即自动平铺，再交换窗口、切分与调整区域。" />

## 操作步骤与默认按键

::: warning 进入后立即排列
按 `Alt+W` 后按 `E`，会立即尝试排列当前候选窗口，后续调整也即时生效。按 `Z` 可撤销，包括进入时的自动排列。
:::

| 按键 | Editor 中的操作 |
| --- | --- |
| `H / J / K / L` | 选择左 / 下 / 上 / 右的区域 |
| `Shift+H/J/K/L` | 朝指定方向切分区域 |
| `Ctrl+H / Ctrl+L` | 缩小 / 增大选中区域宽度 |
| `Ctrl+K / Ctrl+J` | 缩小 / 增大选中区域高度 |
| 两个窗口编号 | 依次选择并交换两个窗口的位置 |
| 反引号（&#96;）后输入区域编号 | 选择区域；已有源窗口时可移入空区域 |
| `X` | 删除区域，不关闭应用窗口 |
| `Z / Shift+Z` | 撤销 / 重做编辑 |
| `Ctrl+S` | 保存布局 |

例如，用方向键选中编辑器所在区域，再按 `Ctrl+L` 增大宽度；相邻区域会一起调整。应用最小尺寸不允许继续缩小时，查看屏幕提示，尝试其他区域或减少分区。

保存操作与演示请看 [Restore · 保存操作](/window-management/restore#save)。

[返回窗口管理总览](/window-management/) · [完整配置参考](/reference/configuration)
