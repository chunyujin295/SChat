# SChat 设计文档

> 局域网内免登录、端到端加密、不可追踪的即时通讯应用（Windows 桌面端）

版本：v1.0 draft　日期：2026-08-25　状态：待评审

---

## 1. 项目概述

### 1.1 目标

| # | 需求 | 设计响应 |
|---|------|----------|
| R1 | 免登录使用 | 首次启动自动生成设备密钥对作为唯一身份，无账号、无服务器 |
| R2 | 可自定义头像、昵称 | 本地资料随发现协议广播，头像本地分发 |
| R3 | 局域网发现好友并建立加密聊天 | UDP 组播自动发现 + TOFU 指纹固定 + X25519 端到端加密会话 |
| R4 | 发送文字 / 图片 / 文件 / 音视频消息 | 统一的分块加密文件通道，支持大文件、进度、断点续传 |
| R5 | 实时音视频通话（二期） | WebRTC P2P，信令复用加密通道，局域网内无需 STUN/TURN |
| R6 | 性能与易用性 | Rust 核心零拷贝分帧；UI 冷启动 < 1s，常驻内存 < 120MB |
| R7 | 现代、简约 UI | React + TailwindCSS，深色优先设计语言，三栏布局 |
| R8 | 仅 Windows | Tauri 2 + WebView2，NSIS 安装包 |

### 1.2 非目标（v1）

- 跨公网通信 / NAT 穿透到外网（仅限局域网/直连网段）
- 群聊（架构预留，单聊先行）
- 消息服务器中转、云端同步
- macOS / Linux 适配

---

## 2. 总体架构

```
┌─────────────────────────────────────────────────────────────┐
│                        SChat 进程                            │
│                                                             │
│  ┌─────────────────────┐        IPC(Command/Event)          │
│  │   前端 WebView2      │◄───────────────────────────────►  │
│  │  React + TS + TW    │    invoke() / emit()               │
│  │  - 联系人 / 会话 UI   │                                   │
│  │  - WebRTC 通话媒体层  │      ┌──────────────────────────┐ │
│  └─────────────────────┘      │       Rust 核心           │ │
│                               │                          │ │
│  ┌── 全局快捷键 / 托盘 / 单实例 ──┤  identity   身份与密钥     │ │
│  └──────────────────────────┤  discovery  UDP 组播发现      │ │
│                               │  transport  TCP 加密会话池   │ │
│                               │  transfer   分块文件引擎     │ │
│                               │  store      SQLite 加密存储  │ │
│                               │  config     设置            │ │
│                               └──────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
         │ UDP :53080 (组播)              TCP :动态端口
         ▼                              ▼
   ┌──────────┐  发现/广播      ┌──────────────────────────┐
   │ 局域网对端 │◄──────────────►│ 对端 SChat：握手→AEAD 帧   │
   └──────────┘                └──────────────────────────┘
```

分层原则：**前端只做渲染与交互，所有网络、加密、持久化都在 Rust 侧完成**；
前端与核心之间只传递已解密的业务数据（内存内），磁盘上永远是密文。

---

## 3. 技术选型

| 层 | 选型 | 版本 | 理由 |
|----|------|------|------|
| 应用框架 | **Tauri 2** | 2.x | 安装包 ~10MB、内存占用低；官方插件覆盖托盘/全局快捷键/单实例/对话框 |
| 核心语言 | **Rust** (stable-msvc) | 1.85+ | 加密/网络性能与内存安全；无 GC 抖动 |
| 异步运行时 | tokio | 1.x | TCP/UDP/定时器/任务取消生态最全 |
| 前端框架 | React 18 + TypeScript | 18.x | 生态成熟，团队熟悉度高 |
| 构建 | Vite | 6.x | 秒级 HMR |
| 样式 | TailwindCSS v4 | 4.x | 原子化 + 设计令牌，v4 无需 config 文件 |
| 状态管理 | zustand | 5.x | 轻量，适配事件驱动型 IPC 推送 |
| 图标/动效 | lucide-react / framer-motion | - | 简约线性图标、克制的微动效 |
| 加密 | RustCrypto 系 (`x25519-dalek` `ed25519-dalek` `chacha20poly1305` `hkdf` `sha2`) | 0.6/0.7 等 | 纯 Rust、审计友好、交叉编译无忧 |
| 存储 | rusqlite (bundled SQLite) | 0.32 | 嵌入式零运维；字段级 AES/ChaCha 加密由应用层实现 |
| 密钥保护 | Windows DPAPI (`CryptProtectData`) | - | 与 Windows 用户账户绑定，免密码保护本地密钥 |
| 实时通话 | WebRTC（WebView2 内建）+ 自有信令 | - | 局域网直连无需 STUN/TURN；Rust 仅转发信令帧 |
| 打包 | NSIS（tauri bundle） | - | 安装包 + WebView2 引导检测 |

**为什么不选 Electron**：包体 100MB+、常驻内存 300MB+、双运行时安全面大，
与本项目的性能与「不可追踪」诉求相悖。

---

## 4. 身份与安全设计

### 4.1 身份模型（免登录）

- 首次启动生成两把密钥：
  - **身份密钥** Ed25519 `(id_sk, id_pk)` —— 长期身份，指纹 = SHA-256(id_pk) 前 16 字节的 Base32（如 `SCAT-7K2F-9QMD-...`），用于展示与比对。
  - **加密子密钥** X25519：由 id_sk 经确定性变换派生（Ed25519→X25519 转换），无需单独保管。
- 昵称默认随机（如 `琥珀猎豹#4832`），头像默认由指纹哈希生成的几何图案头像。
- 所有私钥写入 `%APPDATA%\SChat\identity.bin`，内容用 **DPAPI（CurrentUser）** 封装的 32 字节主密钥做 ChaCha20-Poly1305 加密。换 Windows 用户无法解密。

### 4.2 会话加密

每次 TCP 连接执行一次认证密钥交换（类 Noise KK 简化版）：

```
C → S : HELLO_C { ver, eph_pub_c(X25519), id_pk_c, nonce_c(16B), ts }
S → C : HELLO_S { ver, eph_pub_s, id_pk_s, nonce_s(16B), ts,
                  sig_s = Sign(id_sk_s, H(HELLO_C‖HELLO_S)) }
C → S : sig_c = Sign(id_sk_c, H(HELLO_C‖HELLO_S))
        （C 同时校验 sig_s 且 id_pk_s 指纹 == 本地固定指纹，否则断开并告警）

shared = X25519(eph_sk_c, eph_pub_s)          # 前向保密：临时密钥一次性
okm    = HKDF-SHA256(shared,
                     salt = nonce_c ‖ nonce_s,
                     info = "SCHAT-v1-session", len = 96B)
       → k_c2s(32) ‖ k_s2c(32) ‖ nonce_prefix_c2s(16) ‖ nonce_prefix_s2c(16)
```

- **AEAD**：ChaCha20-Poly1305，nonce = prefix(16B) ‖ u64be(seq)，seq 双向独立递增；
  滑动窗口（64）防重放，乱序即断链。
- **AAD** = `[type(1B)][seq(8B)]`，长度字段不进 AAD 但受 Poly1305 长度校验间接保护。
- **前向保密**：会话密钥基于一次性临时密钥，长期私钥泄露不解密历史流量。

### 4.3 信任模型：TOFU + 指纹固定

- 首次接触某对端 → 将其 `fingerprint` 存入本地信任表（界面展示）。
- 后续连接若指纹变化 → 弹窗红色告警「对方设备指纹已变化」，需手动确认才继续。
- 聊天窗口顶部常驻「已加密 · 指纹尾号 xxxx」徽标，点击可查看双方完整指纹比对。

### 4.4 不可追踪性说明

- 无账号、无手机号/邮箱、无遥测上报、无外联域名（代码层面禁止非局域网出站）。
- 发现包中的身份仅为随机密钥指纹，不含 MAC/主机名/用户名等真实信息。
- 会话密钥周期性轮换（每连接新协商）；聊天记录仅存本机且加密（见 §8）。

---

## 5. 局域网发现协议

### 5.1 传输载体

- 主通道：**UDP 组播** `239.255.53.67:53080`（TTL=1，不出网段）。
- 回退：组播不可用时（部分 AP 屏蔽组播）自动降级为 `255.255.255.255` 定向广播探测。
- 兜底：支持手动「按 IP 添加好友」。

### 5.2 包格式（JSON + Ed25519 签名，≤ 512B）

```jsonc
{
  "v": 1,                      // 协议版本
  "t": "announce",             // announce | query | goodbye
  "inst": "b67d…-uuid",        // 实例 ID（同机重启去重）
  "nick": "琥珀猎豹",
  "fp": "SCAT-7K2F-…",         // 指纹（短形式）
  "pk": "<base64 ed25519 公钥>",
  "port": 54123,               // 本机 TCP 监听端口
  "ava": 12,                   // 头像版本号（变更触发拉取）
  "ts": 1729876543,
  "sig": "<base64>"            // Sign(id_sk, 除 sig 外全部字段的规范化 JSON)
}
```

- 节奏：上线立即发 `query` 触发全网回应 → 之后每 3s 一跳 `announce`（抖动 ±500ms）→ 退出发 `goodbye`。
- 静默判定：12s 未收到心跳标记离线（UI 灰显，消息进入离线队列）。
- 收到未知头像版本或 `ava` 变更 → 向该对端 TCP 发起 `AVA_GET` 拉取头像文件（走加密通道）。

---

## 6. 传输与会话管理

### 6.1 连接策略

- 每个 (本机 ↔ 对端) 维持 **单条长连接**（由指纹寻址，不按 IP）。
- 发起方随机 300–800ms 退避后连接，避免双方同时互连：若两条并存，保留 inst 较小者。
- 心跳：每 10s `PING`，25s 无响应判死重连；指数退避 1s→16s。

### 6.2 帧格式

```
偏移  大小   字段
0     4     length (BE u32, 不含自身; ≤ 262144)
4     1     type
5     8     seq    (BE u64)
13    N     ciphertext (AEAD sealed)
末尾  16    tag (Poly1305)
```

| type | 名称 | 方向 | 说明 |
|------|------|------|------|
| 0x01 | MSG | 双向 | 业务消息体（JSON）：`{mid,kind,body,ts}` |
| 0x02 | ACK | 双向 | `{mid, state: delivered/read}` |
| 0x03 | TYPING | 双向 | 正在输入 |
| 0x04 | FILE_META | 双向 | `{fid,name,size,sha256,mime}` |
| 0x05 | FILE_CHUNK | 双向 | `{fid,off,data}` ≤ 192KB 明文 |
| 0x06 | FILE_ACK | 双向 | `{fid,recv_bytes}` 滑动窗口信用 |
| 0x07 | AVA_GET / AVA_DATA | 双向 | 头像请求/分片 |
| 0x08 | AV_SIGNAL | 双向 | WebRTC 信令透传 `{callId,payload}` |
| 0x09 | PING/PONG/BYE | 双向 | 保活/挂断连接 |

### 6.3 消息可靠性

- TCP 保证字节可靠；应用层以 `mid` 做 **ACK 状态机**：`pending → sent → delivered → read`，UI 以 ⊙ / ✓ / ✓✓ / ✓✓蓝 呈现。
- 对端离线时消息落库 `outbox` 表，心跳恢复后按序补投；超过 72h 或用户删除则放弃。

---

## 7. 文件与媒体传输

- 统一走 FILE_* 通道，图片/语音/视频只是 `mime` 不同的文件 + 消息渲染差异。
- 分块 192KB，信用窗口初始 16 块（≈3MB 在途），接收侧每确认一批回 `FILE_ACK` 补充信用 → 平滑限速防撑爆缓冲。
- 接收流程：写 `%APPDATA%\SChat\files\{yyyyMM}\{fid}.part` → 全量 SHA-256 校验 → 原子改名落位 → 投递消息。
- 断点续传：重连后发送方先发 FILE_META 带 `resume_from`，接收方以已有 .part 大小应答。
- 语音消息：前端 MediaRecorder 录制 webm/opus，上限 10 分钟，气泡内波形播放条。
- 图片：>1080p 自动压缩略图随 FILE_META 附带，点开看原图（lightbox）。

## 8. 实时音视频通话（二期，M5）

- 媒体：WebView2 内建 WebRTC。`getUserMedia()` 取流，`RTCPeerConnection` 直连局域网对端（host candidate 直达，无需 STUN/TURN），Opus + VP8/HW-H264。
- 信令：`AV_SIGNAL` 帧在既有加密通道内透传 SDP offer/answer 与 ICE candidate——**信令本身已端到端加密，无第三方参与**。
- 通话状态机：`idle → ringing(对端响铃UI) → connecting → active / ended / rejected/busy`；忙线直接拒绝并提示。
- UI：全屏遮罩通话面板（头像、计时、静音、开关摄像头、挂断）；视频浮窗可最小化为画中画贴边。

---

## 9. 数据存储设计

位置：`%APPDATA%\SChat\schat.db`（SQLite, WAL）。**敏感字段以 db_key 做 ChaCha20-Poly1305 字段级加密**，db_key 由 DPAPI 封装存放于 `keyring.bin`。

```sql
peers(
  fp TEXT PRIMARY KEY,            -- 指纹
  nickname_enc BLOB, avatar_ver INT, avatar_path TEXT,
  last_ip TEXT, tcp_port INT,
  trusted INT DEFAULT 0, blocked INT DEFAULT 0,
  first_seen INT, last_seen INT
)
messages(
  mid TEXT PRIMARY KEY,           -- uuid
  fp TEXT, dir INT,               -- 0发 1收
  kind TEXT,                      -- text|image|file|audio|video|call
  body_enc BLOB,                  -- 文本内容(密文)；媒体则存 fid
  fid TEXT, ts INT,
  state TEXT                      -- pending|sent|delivered|read|failed
)
files(
  fid TEXT PRIMARY KEY, fp TEXT,
  name_enc BLOB, size INT, sha256 TEXT, mime TEXT,
  path TEXT, dir INT, created_at INT
)
outbox(mid TEXT PRIMARY KEY, fp TEXT, queued_at INT, attempts INT)
settings(k TEXT PRIMARY KEY, v TEXT)   -- 非敏感配置明文 JSON
trust(fp TEXT PRIMARY KEY, pinned_at INT)  -- TOFU 固定记录
```

- 索引：`messages(fp, ts)`；会话列表按 `max(ts)` 聚合查询。
- 「不留痕模式」（设置项，v1.1）：切换后新消息仅存内存 ring buffer，退出清空。
- 清理：文件缓存按容量 LRU（默认 2GB 上限），设置页可视化清理。

---

## 10. UI / UX 设计

### 10.1 设计语言

- 关键词：**深色优先 · 克制 · 高密度但不拥挤**。
- 色板（暗色）：背景 `#0F1115` / 面板 `#161A22` / 描边 `#232936` /
  主色 `#4F8CFF` / 成功 `#34D399` / 危险 `#F87171` / 文本 `#E6EAF2 / #8A93A6`；
  亮色主题同步令牌化（CSS variables 切换）。
- 圆角 10px 卡片 / 18px 气泡；字体栈 `"Segoe UI", "Microsoft YaHei UI"`；基准字号 14px。
- 动效仅用于状态变化（150–200ms ease-out）：列表悬停、气泡入场、通话面板缩放。

### 10.2 布局（三栏）

```
┌────┬──────────────┬───────────────────────────────┐
│导航 │ 会话/联系人    │  聊天区                         │
│ 64 │ 300          │  header: 昵称·在线态·指纹徽标·通话按钮 │
│    │  ┌────────┐  │  ┌───────────────────────────┐ │
│ 💬 │  │搜索     │  │  │ 消息流（按天分组/连续折叠）      │ │
│ 👥 │  ├────────┤  │  │  气泡: 文本/图片/文件卡/语音条    │ │
│ ⚙️ │  │会话列表  │  │  └───────────────────────────┘ │
│    │  │(未读角标)│  │  输入区: 工具条(图片/文件/截图/表情/语音)│
│    │  └────────┘  │  多行输入框 / Enter发送 Shift+Enter换行│
└────┴──────────────┴───────────────────────────────┘
```

### 10.3 关键交互

- **首次启动**：3 步引导（昵称 → 选头像 → 快捷键确认），全部可跳过。
- **发现好友**：「附近的人」实时列表（出现/离开有过渡动画），点头像即可发起会话；首次聊天弹指纹确认卡。
- **发送**：拖拽文件到窗口任意处出现投放遮罩；Ctrl+V 直接粘贴图片；图片发送前可选压缩质量。
- **消息气泡**：右键菜单（复制/撤回[2min内]/另存为/转发/删除）；文件卡显示图标+大小+进度环；失败可点击重发。
- **设置页**：资料（昵称/头像）、外观（主题/壁纸）、快捷键（录制器）、隐私（清空记录/不留痕/黑名单）、关于（指纹完整展示+二维码？否——纯文本比对）。

### 10.4 快捷键

| 场景 | 默认 | 说明 |
|------|------|------|
| 全局：显示/隐藏主窗口 | `Ctrl + Alt + S` | 设置页录制器自定义；冲突时给出提示并要求更换 |
| 全局：截屏发送（v1.1） | 未绑定 | |
| 应用内：搜索 | `Ctrl + F` | |
| 应用内：新会话 | `Ctrl + N` | |
| 应用内：设置 | `Ctrl + ,` | |

实现：`tauri-plugin-global-shortcut`；录制器捕获 `keydown` 合成组合串（如 `Ctrl+Alt+S`），
注册失败（被占用）→ toast 提示；旧组合及时注销。隐藏 = `hide()` 到托盘，再次按下还原并聚焦。

### 10.5 托盘与生命周期

- 关闭按钮 → 最小化到托盘（设置可改为真退出）；托盘左键单击 = 显示/隐藏切换。
- 托盘菜单：显示主窗口 / 在线状态 / 退出（退出前 BYE 所有会话、发 goodbye）。
- 单实例：二次启动唤起已有窗口（`single-instance` 插件）。
- 可选：开机自启（注册表 Run 键，默认关）。

---

## 11. 工程结构

```
SChat/
├─ docs/DESIGN.md
├─ package.json  vite.config.ts  tsconfig.json
├─ index.html
├─ src/                          # 前端
│  ├─ main.tsx  App.tsx
│  ├─ pages/    Chat/ Contacts/ Nearby/ Settings/
│  ├─ components/  Bubble/ FileCard/ VoiceBar/ Lightbox/
│  │               HotkeyRecorder/ AvatarPicker/ CallOverlay/ ...
│  ├─ stores/   session.ts contacts.ts ui.ts      (zustand)
│  ├─ ipc/      bridge.ts (invoke/event 类型封装)
│  └─ styles/   tokens.css
└─ src-tauri/
   ├─ Cargo.toml  tauri.conf.json  capabilities/*.json
   └─ src/
      ├─ lib.rs        # 入口、插件注册、状态管理
      ├─ config.rs     # 设置读写
      ├─ identity.rs   # 密钥生成/DPAPI 封装/指纹
      ├─ crypto.rs     # HKDF/AEAD/重放窗口
      ├─ discovery.rs  # 组播发现 + 广播回退
      ├─ transport.rs  # TCP 监听/拨号/握手/帧循环/会话池
      ├─ transfer.rs   # 文件分块引擎(信用窗口/续传/校验)
      ├─ store.rs      # SQLite + 字段加密 + outbox
      ├─ call.rs       # 通话信令状态机(M5)
      └─ commands.rs   # #[tauri::command] 桥接层
```

核心依赖（Rust）：`tokio(full)`, `serde`, `serde_json`, `ed25519-dalek`, `x25519-dalek`,
`chacha20poly1305`, `hkdf`, `sha2`, `rand`, `rusqlite(bundled)`, `base64`, `uuid`,
`thiserror`, `tracing`, `tauri`, `tauri-plugin-global-shortcut`, `tauri-plugin-single-instance`,
`tauri-plugin-dialog`, `tauri-plugin-autostart`, `windows`(DPAPI)。

前端依赖：`react`, `react-dom`, `zustand`, `tailwindcss(v4)`, `lucide-react`, `framer-motion`, `dayjs`。

---

## 12. 开发里程碑

| 里程碑 | 内容 | 验收标准 |
|--------|------|----------|
| M0 | 工程脚手架（Tauri2+React+TW）、CI 本地构建 | 空壳应用打包运行 |
| M1 | 身份/设置/头像/快捷键/托盘/单实例 | 改昵称头像生效；热键隐藏呼出；关窗驻留托盘 |
| M2 | 发现协议 + 联系人/附近的人 | 两机互见，±3s 上下线感知 |
| M3 | 加密会话 + 文字消息 + 持久化 + 已读回执 | 抓包仅见密文；重启历史可查；离线补投 |
| M4 | 图片/文件/语音消息（进度/续传/预览） | 1GB 文件传输稳定；断网续传成功 |
| M5 | 实时音视频通话 | 两机通话延迟可接受，静音/挂断可用 |
| M6 | 打磨与发布：通知、拖拽、主题、NSIS 安装包 | 安装包 ≤ 15MB，全新 Win10/11 可装可跑 |

## 13. 环境要求与风险

环境（当前机器实测）：Node 24 ✓ / VS2022 MSVC ✓ / WebView2 151 ✓ / **rustup 待安装**（开发第一步执行）。

| 风险 | 影响 | 对策 |
|------|------|------|
| AP 屏蔽组播 | 发现失败 | 广播回退 + 手动 IP 添加 |
| 杀软误报原始套接字 | 安装受阻 | 后期代码签名；白名单指引文档 |
| WebView2 能力差异 | getUserMedia 失败 | 启动自检 + 权限引导页 |
| 同网段多网卡 | 发现地址不准 | 枚举网卡逐一组播宣告 |
| 指纹误固定(TOFU首连被劫持) | 中间人 | 首连 UI 显式确认卡 + 线下核对指纹 |
