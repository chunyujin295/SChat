# SChat

局域网内免登录、端到端加密、不可追踪的即时通讯（Windows 桌面端）。

- 无账号无服务器：设备密钥即身份，首次启动自动生成
- 端到端加密：X25519 临时密钥协商 + ChaCha20-Poly1305，前向保密，抓包只见密文
- 局域网自动发现：UDP 组播（广播回退），昵称/头像本地分发
- 消息类型：文字、图片、文件、语音消息（分块传输、断点续传、SHA-256 校验）
- 已读回执与离线补投；聊天记录字段级加密存储（SQLite + DPAPI 保护密钥）
- 托盘常驻、全局快捷键（默认 `Ctrl+Alt+S`）显示/隐藏窗口，可在设置中自定义

详细设计见 [docs/DESIGN.md](docs/DESIGN.md)。

## 开发环境

- Node.js ≥ 18、Rust (stable-msvc)、VS2022 C++ 生成工具、WebView2 Runtime（Win10/11 一般自带）

```bash
npm install
npm run tauri dev      # 开发调试
npm run tauri build    # 产出 NSIS 安装包（src-tauri/target/release/bundle）
```

## 双机联调

两台机器安装/运行 SChat 后：

1. 左侧「附近」应出现对方（同网段，UDP 53080 组播/广播）
2. 点击对方 → 首次聊天会提示核对指纹（SCAT-XXXX-…），当面比对后点确认
3. 即可互发文字 / 图片 / 文件 / 语音消息

> 单机无法双开测试（单实例限制）；可用虚拟机或第二台电脑验证。

## 安全模型速览

| 威胁 | 对策 |
|------|------|
| 被动抓包 | 所有流量 AEAD 加密，临时密钥前向保密 |
| 中间人 | 握手双向签名 + TOFU 指纹固定 + UI 显式核对 |
| 身份伪造 | 发现包 Ed25519 签名；昵称被抢注时告警 |
| 本地取证 | 私钥 DPAPI 绑定 Windows 用户；消息内容/文件名加密落盘 |

## 目录结构

```
docs/DESIGN.md        设计文档（协议/加密/UI/里程碑）
src/                  React + TypeScript + TailwindCSS 前端
src-tauri/src/        Rust 核心
  identity.rs           密钥与指纹（DPAPI 封装）
  crypto.rs             HKDF / AEAD / 指纹编码
  discovery.rs          UDP 组播发现
  transport.rs          TCP 握手与加密帧会话
  transfer.rs           文件分块引擎（流控/续传/校验）
  store.rs              SQLite 字段级加密存储
  commands.rs           Tauri IPC 命令层
```

## 当前状态（M0–M4 已完成）

- [x] 工程脚手架、托盘、全局快捷键、单实例
- [x] 身份生成、昵称/头像自定义、引导页
- [x] 局域网发现、在线状态、附近列表、头像分发
- [x] 加密会话、文字消息、已读回执、离线补投、加密历史记录
- [x] 图片 / 文件 / 语音消息（进度、续传、校验）
- [ ] 实时音视频通话（M5，WebRTC 信令通道已预留 `call-signal` 事件）
