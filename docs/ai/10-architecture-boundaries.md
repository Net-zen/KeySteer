# 架构边界

当前采用单 crate，模块划分服务于职责与所有权；文件大小或命名本身不是拆分依据。

| 层 | 依赖方向与职责 |
| --- | --- |
| `api` | 跨层词汇底座，不依赖业务实现 |
| `support` | 日志、错误、worker 等通用设施，不依赖业务层 |
| `config` | 描述文档与校验，不装配运行对象 |
| `app` | 应用装配根；configuration 编译 RuntimePlan，catalog 创建模式 |
| `app/runtime` | 使用协议与注入的仓库端口，不读取具体配置存储或原生类型 |
| `modes` / `plugins` | 使用 API 表达行为，不调用平台或具体 presentation |
| `presentation` | 只依赖 API，负责布局与场景生成 |
| `platform` | 实现 Backend，不依赖应用业务实现；common 保持安全 Rust |

状态由明确 owner 持有；共享数据不应模糊释放责任。具体模块可调整，但不要通过全局状态、重复缓存或无意义转发层掩盖依赖问题。

配置编译及重载见 [配置](03-configuration.md)，命令与事件见 [运行时](02-runtime-and-api.md)，资源线程约束见 [后端](06-platform-backends.md)。边界变更同时更新相关实现、架构测试与本目录必要说明。
