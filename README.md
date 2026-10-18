# KeySteer

<p align="center">
  <a href="https://github.com/dccif/KeySteer/actions/workflows/pages.yml"><img alt="Page status" src="https://img.shields.io/github/actions/workflow/status/dccif/KeySteer/pages.yml?branch=main&amp;label=Page&amp;style=flat&amp;logo=github&amp;logoColor=white"></a>
  <a href="https://github.com/dccif/KeySteer/actions/workflows/build.yml"><img alt="Build status" src="https://img.shields.io/github/actions/workflow/status/dccif/KeySteer/build.yml?branch=main&amp;label=Build&amp;style=flat&amp;logo=github&amp;logoColor=white"></a>
  <a href="https://github.com/dccif/KeySteer/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/dccif/KeySteer?display_name=tag&amp;sort=semver&amp;label=Release&amp;style=flat"></a>
  <a href="https://github.com/dccif/KeySteer/blob/main/rust-toolchain.toml"><img alt="Rust 1.99" src="https://img.shields.io/badge/Rust-1.99-dea584?style=flat&amp;logo=rust&amp;logoColor=white"></a>
  <img alt="Windows 10 and 11" src="https://img.shields.io/badge/Windows-10%2F11-0078D4?style=flat&amp;logo=data%3Aimage%2Fsvg%2Bxml%3Bbase64%2CPHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAyNCAyNCI%2BPHBhdGggZmlsbD0iI2ZmZiIgZD0iTTEgMy4yIDEwLjYgMS44djkuNEgxVjMuMlptMTEgNy45VjEuNkwyMyAwdjExLjFIMTJaTTEgMTIuOGg5LjZ2OS40TDEgMjAuOHYtOFptMTEgMGgxMXYxMS4yTDEyIDIyLjR2LTkuNloiLz48L3N2Zz4%3D">
  <img alt="macOS 14 or later" src="https://img.shields.io/badge/macOS-14%2B-000000?style=flat&amp;logo=apple&amp;logoColor=white">
  <a href="https://github.com/dccif/KeySteer/blob/main/LICENSE"><img alt="License: GPL-3.0-or-later" src="https://img.shields.io/github/license/dccif/KeySteer?label=License&amp;style=flat"></a>
</p>

<p align="center">
  <img src="assets/brand/keysteer-wordmark.webp" alt="KeySteer" width="760">
</p>

<p align="center">
  <strong>从点击到分屏，把整个工作区交给键盘。</strong>
</p>

<h3 align="center">
  <strong>简体中文</strong> · <a href="https://github.com/dccif/KeySteer/blob/main/README.en.md">English</a>
</h3>

KeySteer 是 Windows 和 macOS 上的原生键盘操控工具。用键盘移动鼠标、输入标签定位控件，也能分屏、平铺窗口并保存布局。

[下载体验](https://github.com/dccif/KeySteer/releases/latest) · [在线文档](https://dccif.github.io/KeySteer/guide/getting-started) · [在线模拟器](https://dccif.github.io/KeySteer/editor/)

**目录** · [安装](#安装) · [快速上手](#快速上手) · [功能与演示](#功能) · [配置](#配置) · [开发](#运行与打包) · [许可证](#许可证与版权)

## 安装

从 [GitHub Releases](https://github.com/dccif/KeySteer/releases/latest) 下载对应系统与 CPU 架构的 ZIP：

| 系统 | 解压后怎么做 |
| --- | --- |
| Windows 10/11 · x64、ARM64 | 运行 `KeySteer.exe` |
| macOS 14+ · Apple Silicon、Intel | 将 `KeySteer.app` 移入 `/Applications` 后打开 |

macOS 首次使用需授予键盘与鼠标控制权限；UI Hint 视觉识别另需屏幕录制权限。打不开应用或按键无反应时，见 [macOS 安装与授权](https://dccif.github.io/KeySteer/guide/macos)。

<details>
<summary>通过 Scoop 或 Homebrew 安装与升级</summary>

升级前先退出 KeySteer。

### Windows：Scoop

```powershell
scoop bucket add keysteer https://github.com/dccif/KeySteer.git
scoop install keysteer/keysteer

# 升级
scoop update
scoop update keysteer
```

x64 默认使用 AVX2 版，旧 CPU 可从发布页选择兼容版。Scoop 升级保留用户配置与 `workspace.ksw` 布局；从手动安装迁移时，需自行将这些文件复制到 Scoop 安装目录。

### macOS：Homebrew

```bash
brew tap dccif/keysteer https://github.com/dccif/KeySteer.git
brew install --cask dccif/keysteer/keysteer

# 升级
brew update
brew upgrade --cask dccif/keysteer/keysteer
```

自动选择 Apple Silicon 或 Intel 包，升级保留用户配置。安装后仍需完成上面的权限授权。

</details>

## 快速上手

无需创建配置文件。启动后，KeySteer 在 Windows 托盘或 macOS 状态栏待机，平时照常打字。

### 先完成一次点击

1. 按 Windows **左 `Alt+E`** / macOS **`Command+E`**，进入 Normal，然后松开入口键。
2. 按住 `H/J/K/L`，分别向左／下／上／右移动鼠标。
3. 按 `;` 左键点击。
4. 按 `Esc` 返回待机，继续打字。

### 再移动一个窗口

1. 把鼠标放到窗口上，按 Windows **`Alt+W`** / macOS **`Option+W`**，然后松开入口键。
2. 用 `H/J/K/L` 移动窗口。
3. 按 `Z` 撤销一次调整，或按 `Q` 结束并保留结果。

在 Normal 或 Window 中按 **`?`（Shift+/）** 查看可用按键，再按一次收起。字母大写仅便于阅读，写出 `Shift+` 时才需按 Shift。

文档里的 `Primary` 默认指 Windows 左 `Alt`、macOS `Command`，可在配置的 `[key_aliases]` 中修改。

更多练习见 [快速上手](https://dccif.github.io/KeySteer/guide/getting-started)；缩放、居中、跨屏与音量操作见 [窗口管理指南](https://dccif.github.io/KeySteer/window-management/)。

<details>
<summary>常用鼠标按键速查</summary>

### 默认按键

进入 Normal 后使用。

| 按键 | 作用 |
| --- | --- |
| `Caps Lock` / 左 `Shift` / `V` 或 `B` | 按住后精确 / 慢速 / 快速移动，也影响滚动 |
| `M` / `,` | 向下 / 向上滚动 |
| `'` / 右 `Shift` | 右键 / 中键点击 |
| 按住 `N` 再点 `;` | 锁住左键，移动鼠标即可拖拽；单独点 `N` 松开 |
| `Primary+S` | 将鼠标切到下一块显示器 |
| `Q` 或 `Esc` | 返回待机 |

完整键位与拖拽方法见 [Normal 普通模式](https://dccif.github.io/KeySteer/modes/normal)。

</details>

## 功能

需要时再学下面的操作。定位键从 Normal 使用，`A/E/T/R` 从 Window 使用。

| 你想做什么 | 按键 | 指南 |
| --- | --- | --- |
| 用两键快速定位屏幕区域 | `G` | [Grid](https://dccif.github.io/KeySteer/modes/grid) |
| 逐层细分，精确定位小目标 | `F` | [Recursive Grid](https://dccif.github.io/KeySteer/modes/recursive-grid) |
| 输入标签选择控件，搜索文字／简拼、复制信息或取色 | `Primary+F` | [UI Hint](https://dccif.github.io/KeySteer/modes/ui-hint) |
| 把一个窗口放到半屏，再调整比例 | `A` | [Quick](https://dccif.github.io/KeySteer/window-management/quick) |
| 立即平铺多个窗口，再微调区域 | `E` | [Editor](https://dccif.github.io/KeySteer/window-management/editor) |
| 把窗口收成持续存在的标签组 | `T` | [Tabs](https://dccif.github.io/KeySteer/window-management/tabs) |
| 用当前窗口恢复已保存的布局与分组 | `R` | [Restore](https://dccif.github.io/KeySteer/window-management/restore) |

在 Editor 或 Tabs 中按 `Ctrl+S`，填写备注或留空，再按 `Enter` 保存预设。子模式按 `Q` 返回 Window，再按一次回到待机。

### 演示视频

<details>
<summary>鼠标与界面定位：Normal、Grid、Recursive Grid、UI Hint</summary>

### Normal

https://github.com/user-attachments/assets/10c990c4-903c-49fb-b8d7-5441430d3496

### Grid

https://github.com/user-attachments/assets/4ecb749e-d770-43c1-907a-e55a4144a9ca

### Recursive Grid

https://github.com/user-attachments/assets/cb399755-5cde-40a0-ba64-d00c7e581cc6

### UI Hint

https://github.com/user-attachments/assets/71efcae3-eb11-46d0-aba4-0a5df5e9c80c

### UI Hint 搜索：简拼、复制与取色

https://github.com/user-attachments/assets/84d0872d-81f6-4e42-aa7f-767061cef646

</details>

<details>
<summary>窗口管理：移动、分屏、平铺、标签与保存</summary>

### Window：窗口听键盘指挥

https://github.com/user-attachments/assets/a1f38691-f6cf-45bf-b264-03a1a70cd104

### Quick：按 A，快速分屏

https://github.com/user-attachments/assets/e9a278cd-a05a-4931-80cc-d2f832142296

### Editor：按 E，自动平铺再微调

https://github.com/user-attachments/assets/e21b68e3-f970-4e47-b5a2-fec80720b43b

### Tabs：同名程序，自动收成一组

https://github.com/user-attachments/assets/2c1b894d-7402-4b57-9fd1-da03f5bd81a9

### 保存：布局与标签组，都能记下来

https://github.com/user-attachments/assets/9965dbb6-ed80-4071-b636-563563b83b7d

</details>

<details>
<summary>按键提示与图形化配置</summary>

### key_help：忘了快捷键？看一眼就好

https://github.com/user-attachments/assets/9c28c826-d465-4d47-b671-7a275c282a76

### 配置与模拟器：看着完整键盘改键

https://github.com/user-attachments/assets/fbb5f750-fcda-4d3f-92da-1f7667387927

</details>

## 配置

想改键、调速度或换颜色，从托盘或状态栏菜单打开 **Configuration & Simulator...**，将当前配置带入[网页编辑器](https://dccif.github.io/KeySteer/editor/)（需要网络）。

导出为 `keysteer.user.toml`，放入数据目录，再从菜单选择 **Reload Configuration**：

- Windows：`KeySteer.exe` 所在目录。
- macOS `.app`：`~/Library/Application Support/KeySteer/`。

直接改文件时，建议从[完整默认配置](https://dccif.github.io/KeySteer/generated/keysteer.default.toml)开始。自定义 `bindings` 会替换该模式的整张默认按键表，请保留需要的移动、退出和提示键。

配置加载、命令行诊断与排错见[配置文件](https://dccif.github.io/KeySteer/reference/configuration)；动作写法见[模式与动作](https://dccif.github.io/KeySteer/reference/modes-and-actions)。

## 运行与打包

<details>
<summary>从源码运行与贡献</summary>

Rust 版本以 [`rust-toolchain.toml`](https://github.com/dccif/KeySteer/blob/main/rust-toolchain.toml) 为准；文档站使用 Node.js 24。

```bash
cargo run
```

测试和文档站命令见[开发流程](https://dccif.github.io/KeySteer/development/workflow)，发布包制作见[打包说明](https://dccif.github.io/KeySteer/development/workflow#平台打包)。

### 性能

测量方法与输入、绘制约束见[安全与性能约束](https://dccif.github.io/KeySteer/development/safety-and-performance)。

</details>

## 许可证与版权

版权所有 © 2026 dccif。KeySteer 以 **GNU General Public License v3.0 或更高版本（GPL-3.0-or-later）** 发布，完整条款见 [LICENSE](https://github.com/dccif/KeySteer/blob/main/LICENSE)。
