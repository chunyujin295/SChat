# SChat — Secure Chat

<p align="center">
  <img src="./docs/img/icon.png" alt="SChat icon" width="180">
</p>

> **S**ecure **Chat** — 局域网内免登录、端到端加密、不可追踪的即时通讯桌面端。

## 特性

- **无账号无服务器** — 设备密钥即身份，首次启动自动生成 Ed25519 密钥对
- **端到端加密** — X25519 ECDH 密钥协商 + ChaCha20-Poly1305 AEAD，前向保密，网络抓包只见密文
- **局域网自动发现** — UDP 组播（广播回退），无需手动输入 IP，昵称/头像本地分发
- **多类型消息** — 文字、图片、文件、语音消息，支持分块传输、断点续传、SHA-256 完整性校验
- **已读回执** — ⊙ 发送 → ✓ 送达 → ✓✓ 已读，离线消息自动补投
- **加密存储** — SQLite 字段级加密，聊天记录/文件名/头像均加密落盘，私钥受 DPAPI 保护
- **系统集成** — 托盘常驻、全局快捷键 `Ctrl+Alt+S` 显示/隐藏、单实例运行、关闭最小化
- **主题切换** — 深色/浅色主题，全局 CSS 变量一键切换

## 快速开始

### 环境要求

| 依赖 | 版本 | 用途 |
|------|------|------|
| Node.js | ≥ 18（推荐 LTS） | 前端构建 + Tauri CLI |
| Rust | stable-msvc | 后端编译 |
| Visual Studio Build Tools | 2022（C++ 生成工具） | MSVC 链接器 + Windows SDK |
| WebView2 Runtime | Win10/11 一般自带 | 渲染 WebView |

### Windows 构建环境安装

以下在 Windows 10/11 上从零搭建，按顺序安装即可。

#### 1. Node.js

前往 <https://nodejs.org> 下载 LTS 安装包（`.msi`），一路默认安装。

```bash
node -v   # 应输出 v18 或更高
npm -v
```

#### 2. Rust（MSVC 工具链）

前往 <https://rustup.rs> 下载 `rustup-init.exe` 并运行，默认安装即可（会自动选择 `x86_64-pc-windows-msvc` 目标）。

```bash
rustc -V
cargo -V
```

> 安装器会自动把 `%USERPROFILE%\.cargo\bin` 加入 PATH；本项目的 `dev.bat` / `build.bat` 运行时也会再补上该路径，双保险。

#### 3. Visual Studio Build Tools（C++ 生成工具）

Tauri 后端需要 MSVC 链接器（`link.exe`）和 Windows SDK，Rust 本身不带这些，必须单独安装：

- 前往 <https://visualstudio.microsoft.com/visual-cpp-build-tools/> 下载 **Build Tools for Visual Studio 2022**
- 启动安装器，勾选工作负载 **「使用 C++ 的桌面开发」**（Desktop development with C++）
- （也可安装完整版 Visual Studio Community，勾选同样的 C++ 工作负载）

安装完成后 `rustc` 会通过 `vswhere` 自动定位链接器，无需手动配置。

#### 4. WebView2 Runtime

Windows 10/11 通常已预装（Edge 自带）。若运行产物报「找不到 WebView2」，前往 <https://developer.microsoft.com/microsoft-edge/webview2/> 下载 Evergreen 版安装。

### 安装依赖

```bash
npm install
```

### 开发调试

```bash
# 方式一：命令行
npm run tauri dev

# 方式二：双击脚本
scripts\dev.bat
```

### 构建发布

```bash
# 方式一：命令行
npm run tauri build

# 方式二：双击脚本（自动清理 + 版本同步 + 构建）
scripts\build.bat
```

构建完成后产出两个文件：

| 产物 | 路径 |
|------|------|
| NSIS 安装包 | `src-tauri\target\release\bundle\nsis\SChat_<version>_x64-setup.exe` |
| 裸程序 | `src-tauri\target\release\schat.exe` |

构建成功后会自动打开安装包所在目录。

### NSIS 工具链下载（打包安装包时）

`tauri build` 首次打包 **NSIS 安装包**时，会从 GitHub 自动下载 NSIS 工具链（`nsis-3.11.zip` 与 `nsis_tauri_utils.dll`），缓存到 `%LOCALAPPDATA%\tauri\NSIS`。若下载超时（常见于国内网络），可选下面任一方案：

**方案 A：挂代理（最简单）**

打包器会自动读取系统代理环境变量：

```bash
set HTTPS_PROXY=http://127.0.0.1:7890
set HTTP_PROXY=http://127.0.0.1:7890
npm run tauri build
```

**方案 B：改用 GitHub 镜像**

打包器支持两个镜像环境变量（二选一）：

```bash
# 基础镜像：把完整 github 地址拼到镜像站之后转发
set TAURI_BUNDLER_TOOLS_GITHUB_MIRROR=https://<你的镜像站>

# 模板镜像：用 <owner>/<repo>/<version>/<asset> 占位符拼出下载地址
set TAURI_BUNDLER_TOOLS_GITHUB_MIRROR_TEMPLATE=https://<你的镜像站>/<owner>/<repo>/releases/download/<version>/<asset>
```

> 镜像站可用性与地址随时变化，请替换成当前可用的镜像。

**方案 C：手动预置缓存（离线可用）**

按下面步骤把工具链直接放进缓存目录，打包器检测到后就不再联网下载。在 PowerShell 中执行：

```powershell
$root = "$env:LOCALAPPDATA\tauri"
New-Item -ItemType Directory -Force -Path $root | Out-Null

# 1) 下载 NSIS 工具链并解压为 NSIS 目录
curl.exe -L -o "$root\nsis.zip" "https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.11/nsis-3.11.zip"
Expand-Archive -Path "$root\nsis.zip" -DestinationPath $root -Force
Rename-Item "$root\nsis-3.11" "$root\NSIS"

# 2) 下载 tauri 的 NSIS 插件
New-Item -ItemType Directory -Force -Path "$root\NSIS\Plugins\x86-unicode\additional" | Out-Null
curl.exe -L -o "$root\NSIS\Plugins\x86-unicode\additional\nsis_tauri_utils.dll" "https://github.com/tauri-apps/nsis-tauri-utils/releases/download/nsis_tauri_utils-v0.5.3/nsis_tauri_utils.dll"

# 3) 校验（可选，SHA1 与打包器内置值一致）
Get-FileHash -Algorithm SHA1 "$root\nsis.zip"          # 期望 EF7FF767E5CBD9EDD22ADD3A32C9B8F4500BB10D
Get-FileHash -Algorithm SHA1 "$root\NSIS\Plugins\x86-unicode\additional\nsis_tauri_utils.dll"  # 期望 75197FEE3C6A814FE035788D1C34EAD39349B860
```

> 校验通过后 `nsis.zip` 可删除，`NSIS` 目录保留即可；此缓存持久有效，之后打包不再联网。

**方案 D：跳过 NSIS，只出裸程序**

若暂时不需要安装包：

```bash
npm run tauri build -- --no-bundle
```

### 版本管理

版本号统一在 `scripts\version.txt` 中维护，构建时自动同步到：
- `package.json`
- `src-tauri/tauri.conf.json`
- `src-tauri/Cargo.toml`

```bash
# 修改版本
echo v1.0.0 > scripts\version.txt

# 构建（自动应用版本号）
scripts\build.bat
# → SChat_1.0.0_x64-setup.exe
```

### 脚本说明

`scripts/` 目录下提供了一键脚本，双击即可执行，无需手动敲命令：

| 脚本 | 功能 |
|------|------|
| `dev.bat` | 启动开发模式（自动安装依赖 + `tauri dev`） |
| `build.bat` | 发布构建（自动清理旧产物 → 读取 `version.txt` 同步版本 → 构建 → 打开产出目录） |
| `version.txt` | 唯一版本号来源，格式 `v0.0.1`，构建时自动同步到 `package.json` / `tauri.conf.json` / `Cargo.toml` |

> 脚本会自动将 `~\.cargo\bin` 加入 PATH，首次运行自动安装 `node_modules`，无需额外配置。

## 双机联调

1. 两台电脑接入同一局域网
2. 启动 SChat，左侧「附近」tab 应自动发现对方
3. 点击对方发起聊天 → 首次会提示核对指纹（`SCAT-XXXX-…`）
4. 当面比对指纹一致后，双方点击「我已核对，确认」
5. 即可互发文字 / 图片 / 文件 / 语音消息

> ⚠ 单机无法双开测试（单实例限制），需使用虚拟机或第二台电脑。

## 安全模型

| 威胁 | 对策 |
|------|------|
| 被动抓包 | 所有流量 AEAD 加密，临时密钥前向保密 |
| 中间人攻击 | 握手双向签名 + TOFU 指纹固定 + UI 显式核对 |
| 身份伪造 | 发现包 Ed25519 签名验证；昵称被抢注时告警 |
| 本地取证 | 私钥 DPAPI 绑定 Windows 用户；消息内容/文件名加密落盘 |
| 重放攻击 | 帧级递增序号 + 滑动窗口拒绝旧帧 |

## 技术栈

```
前端    React 18 · TypeScript · TailwindCSS v4 · Zustand · Lucide Icons
后端    Rust · Tauri 2 · tokio · rusqlite · ed25519-dalek · x25519-dalek · chacha20poly1305
协议    UDP 组播发现 · TCP 长连接 · X25519 ECDH · ChaCha20-Poly1305 AEAD · HKDF-SHA256
存储    SQLite (WAL) · 字段级加密 · DPAPI 密钥保护
```

## 项目结构

```
SChat/
├── docs/
│   ├── DESIGN.md                设计文档（协议/加密/UI/里程碑）
│   └── BUGFIX-file-image-preview.md  故障复盘（文件/图片无法预览）
├── scripts/
│   ├── dev.bat                 开发调试脚本
│   ├── build.bat               一键构建脚本
│   ├── sync-version.cjs        版本同步脚本（build.bat 调用）
│   └── version.txt             版本号
├── src/                        React + TypeScript 前端
│   ├── api.ts                  Tauri IPC 封装
│   ├── store.ts                Zustand 状态管理
│   ├── types.ts                类型定义
│   └── components/
│       ├── ChatPane.tsx        聊天窗口
│       ├── ListPane.tsx        联系人列表
│       ├── NavRail.tsx         导航栏
│       ├── SettingsModal.tsx   设置面板
│       └── ui.tsx              通用组件
└── src-tauri/src/              Rust 核心
    ├── identity.rs             密钥与指纹（DPAPI）
    ├── crypto.rs               HKDF / AEAD / 指纹编码
    ├── discovery.rs            UDP 组播发现
    ├── transport.rs            TCP 握手与加密会话
    ├── transfer.rs             文件分块引擎
    ├── store.rs                SQLite 加密存储
    ├── core.rs                 共享状态与会话管理
    └── commands.rs             Tauri IPC 命令层
```

## 里程碑

- [x] **M0** 工程脚手架 — Vite + React + Tauri 2 骨架、图标、首次构建
- [x] **M1** 身份与设置 — 密钥/指纹/DPAPI、资料编辑、全局快捷键、托盘
- [x] **M2** 发现与联系人 — UDP 组播、在线状态、附近列表、头像分发
- [x] **M3** 加密会话 — TCP 握手/AEAD、文字消息、已读回执、离线补投
- [x] **M4** 媒体传输 — 图片/文件/语音、分块流控、断点续传、SHA-256 校验
- [ ] **M5** 实时音视频 — WebRTC 信令通道已预留（`call-signal` 事件）

## 变更记录

| 日期 | 版本 | 内容 |
|------|------|------|
| 2026-08-27 | 0.2.0 | 修复：发送的文件无法打开、发送的图片双方都看不到（`files` 表读取列顺序错位）。详见 [docs/BUGFIX-file-image-preview.md](docs/BUGFIX-file-image-preview.md) |

## License

Private — 仅供个人使用。
