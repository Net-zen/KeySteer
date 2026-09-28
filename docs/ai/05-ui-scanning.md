# UI 扫描

链路：Mode 发 `ScanUi` → Backend 调度原生扫描 → 合并与去重 → 分批 `UiScanned` → Hint 会话更新。

## 入口

- 协议：`src/api/command.rs`；消费端：`src/modes/hint/`。
- Windows：`src/platform/windows/accessibility.rs`、`src/platform/windows/ui_scan.rs`、`src/platform/windows/vision/`。
- macOS：`src/platform/macos/accessibility.rs`、`src/platform/macos/ui_scan.rs`、`src/platform/macos/vision.rs`。
- 共享算法：`src/platform/common/scan_mailbox.rs`、`src/platform/common/scan_accumulator.rs`、`src/platform/common/spatial_index.rs`、`src/platform/common/contour/`。

## 必须保持

- 扫描异步、可按 scan id 取消，流式交付 Partial；Engine 不等待完整扫描。
- 结果验证 scan id 和目标上下文；旧请求不能覆盖新会话。
- 终态与数据分开理解：超时不等于清空已有目标；重扫和退出清理旧会话资源。
- 遍历、候选数量、图像和在途工作有明确边界；取消不能只取消显示而让旧任务无限运行。
- 去重及多来源融合保持稳定语义，不依赖哈希迭代顺序。
- UIA/AX 等原生引用由对应平台线程拥有，跨线程只交付允许传递的数据。

策略、范围、超时与目标上限查配置和代码；扫描算法可调整，但应覆盖取消、增量顺序、重复目标和跨屏场景。
