# 配置与持久化

## 查找入口

- `src/config/`：文档模型、默认值、别名、校验和保留注释的写回。
- `src/app/configuration.rs`：配置编译边界；`src/app/mode_catalog.rs`：模式 Settings 与路由装配。
- `src/api/input.rs`、`src/api/binding.rs`：键与动作语法。
- `src/app/paths.rs`、`src/app/restart.rs`、`src/app/preset_store.rs`：路径、重载进程、工作区存储。

## 保留的语义

- 发布默认配置与内置默认值一致；未知字段、非法组合和冲突应明确报错。
- 模式只接收编译后的 Settings，不读取 TOML；别名、继承、应用覆盖和临时模式共用解析规则。
- 字符输入与物理键身份分开；保持修饰键匹配、长按重复和 Down/Up 配对。
- 生产 Reload 先验证并准备新进程，失败保留旧实例；旧实例有序退出后交接已验证配置。进程内 `set_config` 仍原子替换完整计划。
- 配置与工作区 I/O 不阻塞输入循环；写入失败不覆盖有效数据，格式变更需考虑兼容性。
- macOS 打包应用的数据在 `~/Library/Application Support/KeySteer/`；portable 使用可执行文件目录。数据目录不能取进程工作目录；显式相对配置路径另按 CLI 规则解析。

字段、默认键位和格式细节直接查 `keysteer.default.toml`、解析代码及测试。修改用户配置语义时同步用户参考和网页编辑器适用部分。

`scroll.speed` 是普通长按滚动的像素/秒，默认 500，独立于短按距离；接受有限非负小数，0 为仅短按。未发布的 `steps_per_second` 已移除，不提供兼容转换。模式装配时将短按距离和每秒速度编译为 `ScrollSettings`，半页／整页仅按次执行，其连续速度为 0。模式按键变化时缓存方向速度与持续手势状态，帧热路径只做时间积分，不读取配置或解析字段。网页 `docs/.vitepress/simulator/scroll.ts` 同样预编译速度，控件与移动速度一样直接编辑数值，不设额外速度档位，导出仅使用 `speed`，保留速度修饰键和 macOS 方向设置。

窗口卡片共用 `[window.card]`；选中底色、边框颜色与线宽可由 `selected_background_color`、`selected_border_color`、`selected_border_width` 覆盖。明暗主题薄荷绿及 1.5 线宽为内置默认值，默认配置和原生导出不写入这些默认字段；网页控件显示默认值，但只在编辑后生成对应覆盖，重置删除覆盖。颜色支持 `#RRGGBBAA` 和完整的浅深主题表，线宽为 0–20，0 隐藏卡片选中边框；勾选标记仍保留。原生在装配时预编译样式，网页编辑器、普通／选中对照与交互预览使用同一配置。

UIHint 搜索样式缺省时使用 `UiHint::default()` 的对应面板默认值。部分 `search_input_ui`／`search_info_ui` 表也在配置反序列化阶段补齐各自默认字段；信息面板不能退回输入框的通用宽度和屏幕锚点。补齐后沿用共享校验和启动样式编译，运行时不解析配置。

`ui_hint.search_edit_keys` 按动作配置搜索编辑快捷键，稀疏表覆盖默认值；与信息复制键一起校验冲突，并在启动阶段解析为 KeyChord。编辑键、复制键与 `ui_hint.search_bindings` 一起在启动时展开组合键内部的配置别名；`primary` 遵循平台别名覆盖（发布 Windows 为 left_alt），也允许显式 cmd、ctrl。

搜索编辑键支持以空格分隔同一动作的多个快捷键，启动时展开与校验，不在输入热路径解析。默认 accept 为 `enter / primary+q`，自定义值替换该动作全部默认键。

颜色原文只保留在配置模型中，用于校验和序列化。网格、Hint、按键帮助与模式指示器在装配时转换为 `CompiledColor` 和 `style::compiled` 样式，运行时只选择浅／深色数值；未配置值及无效程序化颜色仍保留原有回退语义。搜索面板、窗口卡片和快速切换沿用已有整样式编译。透明度、对比度和场景默认色在绘制时派生，不因提前解析而冻结；主题变化选择另一组数值，配置重载重建整个计划。

Window 多选入口与清空沿用 `window.bindings` 的 `window_multi_select`／`window_multi_clear`；输入期间由 `window.multi_select.bindings` 覆盖确认及编辑键，默认 Ctrl／Enter 确认、primary+h/l 移动输入光标。编辑键显式写在该表，支持 arrow_left/right、home/end、backspace/delete 及 Shift 方向选区，不从 Normal 复制配置。空格仍表示多个快捷键别名，也不改变 `window.target`。

搜索点位操作使用 `ui_hint.search_bindings` 的“按键 = 动作”表（`point_toggle` / `point_next` / `color_next`；point_next 默认 Tab，仅调整期间生效），显式表替换默认映射；点位样式和格式顺序在 `ui_hint.search_point`，缺省字段不展开到导出配置。网页控件沿用此替换语义。四栏多点展示由 search_point.field_modes 固定长度数组配置，concat 拼接初始目标信息、switch 跟随当前检查点，默认 [concat, concat, switch, switch]；默认值不展开到导出。颜色仅在唯一可见搜索结果时生效，不执行多点颜色拼接；第四项继续保留配置兼容。反序列化直接形成四项枚举，catalog 复制到 Settings，模式只接收编译后的按键、颜色、字段行为与格式枚举，运行时不解析字符串。

四个搜索信息复制键继续使用 `ui_hint.search_copy_keys` 独立数组配置，默认 Ctrl+1/2/3/4，两平台一致；显式 primary 仍按别名展开。`search_point.color_preview` 控制 Color 标题旁色块的开关、宽高、水平／垂直偏移与边框线宽；默认表及未改字段不导出。网页外观页提供同范围控件和预览，重置删除覆盖。

Point 输入框的 `search_point.input_background_color`／`input_border_color` 为可选主题颜色；缺省背景采用窗口多选卡片同款浅／深绿色，边框继承普通搜索框。两项在装配时预编译，默认值不写入配置；网页外观页同时编辑普通搜索框和 Point 覆盖并预览对照。
