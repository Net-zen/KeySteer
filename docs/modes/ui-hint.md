# UI Hint 标签模式

<script setup>
import ModeVideo from '../.vitepress/components/ModeVideo'
</script>

按钮太小、鼠标太远？UI Hint 给按钮、链接、菜单和输入框贴上短标签。输入标签，鼠标就到目标旁边。

<ModeVideo
  file="uihint.mp4"
  title="UI Hint 模式演示"
  description="展示扫描界面元素、输入标签筛选并把鼠标定位到目标控件。"
/>

从 Normal 按 `f` 进入。

::: tip 定位后再点击
默认只移动鼠标，随后回到 Normal；用 `;` 左击确认即可。
:::

## 默认操作

| 按键 | 作用 |
| --- | --- |
| 标签字符 | 缩小范围；完整匹配后定位 |
| `Shift` | 切换重叠的标签 |
| `Primary+R` | 重新识别当前界面 |
| 按住 `Primary` | 临时使用 Normal 的移动、滚动和点击 |
| `Primary+Q` / `Esc` | 返回 Normal |

## 扫描方式

先用默认的 `hybrid` 即可。标签会分批出现，看到目标就可以输入。

| 方式 | 适合什么情况 |
| --- | --- |
| `hybrid` | 默认选择：结合控件信息、文字识别（OCR）和图形轮廓 |
| `axtree` | 标准按钮、菜单等有辅助功能信息的控件 |
| `vision` | 自绘界面，结合文字和轮廓识别 |
| `contour` | 只找图形轮廓；没有文字名称，不能按名称搜索 |

默认 `scan_scope = "all"`，扫描**全部显示器**。可用 `"window"` 优先扫描鼠标下的窗口，`"active"` 优先扫描激活窗口，或 `"screen"` 扫描鼠标所在整屏。窗口范围找不到首选窗口时依次尝试另一个窗口和鼠标所在屏幕。整屏范围包括各程序当前可见的界面，跳过被其他窗口完全遮住的内容。选定范围后，空结果不会触发换范围重扫。

Windows 和 macOS 都优先处理扫描开始时鼠标所在屏幕、窗口及周围区域，识别结果陆续显示，再补齐其余范围；`screen` 和 `all` 仍扫描完整范围。

切换目标窗口或显示器时会重新识别；`all` 范围内跨屏移动保留已有标签，所有显示器统一编号，完整标签不会因跨屏而重复。普通鼠标移动不会持续重扫。需要更新标签时，按 `Primary+R`。目标数量动态增长，不在 1 万条处截断；更多可见内容仍会增加识别时间和内存。

::: info macOS 需要两项权限
键盘控制需要**设备控制和数据访问**（macOS 27；旧称“辅助功能”）；视觉识别还需要**录屏与系统录音**（旧称“屏幕录制”）。见 [macOS 授权步骤](/guide/macos)。
:::

## 搜索与结果预览

<ModeVideo
  file="uihint-search.mp4"
  title="UI Hint 搜索、复制与取色"
  description="22 秒中英双语演示，纯字幕：简拼搜索、切换结果、复制信息、微调点位与取色。"
/>

### 搜索一个目标

1. 按 `/`，输入文字、中文简拼或标签，比如 `复制`、`fz`、`text_field`。
2. 面板自动预览最佳匹配；按 `Tab` 看下一个结果。
3. 按 `Enter`、`/` 或 `Primary+Q` 移动到当前预览点，继续留在 UI Hint。`Esc` 取消搜索。

切换预览不会移动鼠标；继续输入时，会重新显示最佳匹配。`@la` 或 `la@` 只搜索标签，避免和文字混在一起。用空格可以选择多个目标，例如 `@la @ka`。

::: tip 当前预览会变绿
绿色标签表示正在预览或调整的目标；多个点还停在平均中心时，不突出某一个标签。可在网页配置器的 **UI Hint → 外观 → 当前标签高亮** 修改样式。
:::

### 复制文字、坐标和颜色

| 按键 | 操作 |
| --- | --- |
| `Ctrl+1 / 2 / 3 / 4` | 分别复制文字、辅助信息、坐标、颜色 |
| 轻按 `Ctrl` | 在搜索输入和点位调整之间切换 |
| `H/J/K/L`（调整时） | 微调取色位置 |
| `Ctrl+Shift+4` | 切换 HEX、RGB、HSL 颜色格式 |

复制信息成功后会确认当前点并退出搜索；没有内容或复制失败时保留搜索。Windows 和 macOS 都使用上述 Ctrl 键位。

### 调整搜索顺序和按键

结果先按匹配程度排序：**完整匹配 → 前缀 → 包含**。下面的 `search_match_priority` 只决定同等匹配程度下的顺序，三项各写一次：`pinyin` 简拼、`text` 文字和控件类型、`label` 标签。

```toml
[ui_hint]
search_match_priority = ["pinyin", "text", "label"]

[ui_hint.search_bindings]
ctrl = "point_toggle"
tab = "point_next" # 可改为 f9、alt+f9 等
"ctrl+shift+4" = "color_next"
```

`search_bindings` 中，`point_toggle` 切换点位调整，`point_next` 切换结果或已选点，`color_next` 切换颜色格式。自定义这张表会替换默认表，上例保留了三个默认操作。

搜索框编辑键另用 `[ui_hint.search_edit_keys]`，例如 `"enter / primary+q" = "accept"`；左侧空格分隔多个快捷键，省略的编辑动作保留默认键。

完整多选、复制和样式参数见 [搜索与信息复制](/reference/modes-and-actions#uihint-搜索与信息复制)。

## 常用配置

```toml
[ui_hint]
strategy = "hybrid"
hint_characters = "asdfghjkl"
scan_timeout_ms = 2500
scan_retry_count = 1
scan_retry_delay_ms = 200
visible_check_enabled = false
placement = "bottom"
label_x_offset = 0
label_y_offset = -8
clickable_roles = ["button", "link", "checkbox", "text_field", "menu_item"]

[ui_hint.lifecycle]
after_finish = "normal"
after_click = "normal"
```

| 参数 | 怎么用 |
| --- | --- |
| `hint_characters` | 生成标签时使用的字符 |
| `scan_timeout_ms` | 每次识别最多等待多久，单位毫秒 |
| `scan_retry_count` / `scan_retry_delay_ms` | 没有标签时的重试次数和间隔 |
| `clickable_roles` | 限制辅助功能来源的控件类型；不影响 OCR 和轮廓 |
| `placement` / `label_x_offset` / `label_y_offset` | 标签相对目标的位置和偏移 |
| `after_finish` / `after_click` | 定位完成或点击后转到哪里 |

大页面识别不全时，可增加超时和重试次数。切换窗口会立即重新识别，不占用空结果的重试次数。

## 视觉样式

也可以在[配置与模拟器](/editor/)中直接预览和调整。

```toml
[ui_hint.ui]
font_size = 17
padding_x = -1
padding_y = -1
border_width = 1

[ui_hint.boundary_highlight]
enabled = false
border_width = 1

[ui_hint.search_input_ui]
position_mode = "screen"
position = ["95%", "50%", "5%", "50%"]
width = 320
```

`ui` 调整标签字号和边框；`boundary_highlight` 控制目标区域描边；`search_input_ui` 调整搜索框。面板位置的写法见 [搜索面板定位](/reference/configuration#搜索面板定位)。

## 视觉识别建议

| 遇到的情况 | 可以怎么试 |
| --- | --- |
| 自绘界面缺少控件信息 | 改用 `strategy = "vision"` |
| 标准控件标签太多 | 缩小 `clickable_roles` 范围 |
| Windows 文字识别不理想 | 检查 Windows 已安装的 OCR 识别语言；KeySteer 使用系统 OCR |
| macOS 没有视觉标签或颜色 | 检查录屏权限，授权后重启 KeySteer |
