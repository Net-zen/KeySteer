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

UIHint 搜索样式缺省时使用 `UiHint::default()` 的对应面板默认值。部分 `search_input_ui`／`search_info_ui` 表也在配置反序列化阶段补齐各自默认字段；信息面板不能退回输入框的通用宽度和屏幕锚点。补齐后沿用共享校验和启动样式编译，运行时不解析配置。
