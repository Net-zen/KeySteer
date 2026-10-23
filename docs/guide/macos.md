# macOS

KeySteer 支持 macOS 14 及以上。安装后先授予键盘与鼠标控制权限（旧称“辅助功能”），再回到[快速上手](/guide/getting-started)试一次点击。

## Homebrew 安装与升级

```bash
brew tap dccif/keysteer https://github.com/dccif/KeySteer.git
brew install --cask dccif/keysteer/keysteer
```

自动选择 Apple Silicon 或 Intel 包。升级前退出 KeySteer，然后运行：

```bash
brew update
brew upgrade --cask dccif/keysteer/keysteer
```

用户配置保存在下方的数据目录中，升级时保留。安装后仍需完成下方的权限授权。

## GitHub Release 手动安装

从 [官方 GitHub Releases](https://github.com/dccif/KeySteer/releases/latest) 下载对应架构的 ZIP，解压后将 `KeySteer.app` 移入 `/Applications`。如果 Gatekeeper 提示应用无法打开，请先确认下载来源，然后运行：

```bash
sudo xattr -cr /Applications/KeySteer.app
```

## 首次授权（推荐）

1. 打开 `/Applications/KeySteer.app`。
2. 前往“系统设置 → 隐私与安全”（部分版本显示为“隐私与安全性”），按版本打开对应权限：

   - **macOS 27**：设备控制和数据访问（Device Control and Data Access）。
   - **macOS 14–26**：辅助功能（Accessibility）。

3. 允许 KeySteer 控制电脑。
4. 重新启动 KeySteer，并按 `Command+E` 测试。

找不到权限项时，可在系统设置中搜索“设备控制”或“辅助功能”，打开应用授权列表；没有 KeySteer 时，点 `+` 添加 `/Applications/KeySteer.app`。应用或旧提示中提到的“辅助功能权限”，在 macOS 27 对应上述新名称。

`UI Hint` 的视觉识别还需要同一隐私页面内的**录屏与系统录音**权限（旧版称“屏幕录制”；英文 Screen & System Audio Recording）。只使用 Grid 或 Recursive Grid 时不需要读取屏幕内容，但仍需上述键盘与鼠标控制权限。

::: tip 找不到视觉标签？
先检查“录屏与系统录音”权限，授权后重启 KeySteer。
:::

## 文件位置

打包的 `.app` 使用：

```text
~/Library/Application Support/KeySteer/
```

其中包括：

- `keysteer.<名称>.toml`：配置文件。
- `keysteer.log`：运行日志。
- `keysteer.log.1`、`.2`、`.3`：轮换日志。

## 滚动方向

如果你使用自然滚动，默认已开启垂直方向反转；可以在配置中调整：

```toml
[platform.macos.scroll]
invert_horizontal = false
invert_vertical = true
```

## 权限问题

如果顶部状态图标正常，但按键没有反应：

1. 确认“设备控制和数据访问”（旧版为“辅助功能”）列表中授权的是 `KeySteer.app`。
2. 关闭并重新打开 `KeySteer`。
3. 运行 `keysteer --doctor` 查看键盘是否可用。
4. 仍有问题时查看数据目录中的 `keysteer.log`。

::: tip 升级后按键失效
可在权限列表中移除旧的 KeySteer，重新添加 `/Applications/KeySteer.app`，再重启程序。
:::

## 顶部状态栏图标

KeySteer 运行后，屏幕顶部右侧会出现状态图标。点击可暂停控制、重载配置、检查更新或退出。平时从这里管理程序，Dock 中没有单独图标。
