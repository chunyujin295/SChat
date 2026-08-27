# 故障复盘：发送文件打不开、发送的图片双方都看不到

> 状态：已修复　影响版本：≤ 0.1.5　修复日期：2026-08-27

## 1. 现象

两个看似无关、实则同源的问题在文件/图片消息上同时出现：

| # | 现象 | 触发路径 |
|---|------|----------|
| 1 | 发送文件后，在聊天页面**点击文件卡片无法打开** | 发送方与接收方均复现 |
| 2 | 发送图片后，**发送方和接收方都看不到图片**（只剩灰色占位框） | 图片消息渲染 |

文字消息、语音消息不受影响。

## 2. 排查过程

### 2.1 定位到「文件路径丢失」

前端渲染逻辑都依赖消息对象上的 `fpath` 字段：

- 图片：[`ImageMsg`](../src/components/ChatPane.tsx) 中 `if (!m.fpath || m.progress != null)` 成立时，只渲染灰色占位框，不加载 `<img>`。
- 文件：[`FileCard`](../src/components/ChatPane.tsx) 中 `const done = !!m.fpath && m.progress == null`，`done` 为 `false` 时点击不会调用 `openPath`。

据此推断：**消息对象的 `fpath`（以及 `fname`/`fsize`/`mime`）没有被正确填充**。

### 2.2 定位到 `fpath` 的装配点

`fpath` 在 Rust 侧由 [`core::build_msg_view`](../src-tauri/src/core.rs) 装配：

```rust
if let Some(fid) = &row.fid {
    if let Some((name, _sha, size, mime, _kind, _dir, path)) =
        core.db.file_info(fid).ok().flatten()
    {
        v["fid"]   = ...;
        v["fname"] = ...;   // name
        v["fsize"] = ...;   // size
        v["mime"]  = ...;   // mime
        v["fpath"] = ...;   // path
    } else {
        v["fid"] = ...;     // ← 只剩 fid，其余字段全部缺失
    }
}
```

问题被 `file_info(fid).ok().flatten()` 静默吞掉了：只要 `file_info` 返回 `Err`，就会走 `else` 分支，消息就只剩 `fid` 而没有 `fpath`。

### 2.3 找到真正的根因

[`store.rs`](../src-tauri/src/store.rs) 的 `file_info` 里，SQL 的 **SELECT 列顺序** 与 **读取/解构顺序** 不一致：

```rust
// SQL 返回的列顺序：…, kind, path, dir
"SELECT name_enc,size,sha,mime,kind,path,dir FROM files WHERE fid=?1"

// 读取时的索引（对号入座）：
r.get::<_, Vec<u8>>(0)?,   // name_enc  ✓
r.get::<_, i64>(1)?,       // size      ✓
r.get::<_, String>(2)?,    // sha       ✓
r.get::<_, String>(3)?,    // mime      ✓
r.get::<_, String>(4)?,    // kind      ✓
r.get::<_, i64>(5)? as i32,// ← 期望 dir，实际拿到的是 path（文本列）
r.get::<_, String>(6)?,    // ← 期望 path，实际拿到的是 dir（整数列）
```

第 6 列 `path` 是 `C:\Users\...\files\202608\xxx.png` 这样的**文本**，却被当成 `i64`（整数）读取，rusqlite 类型转换失败 → `file_info` 抛错 → `build_msg_view` 里 `.ok().flatten()` 得到 `None` → `fpath` 丢失。

### 2.4 为什么写入是对的、读取却错了

写入端 [`upsert_file_meta`](../src-tauri/src/store.rs) 的 INSERT 显式列名且顺序正确：

```rust
INSERT OR REPLACE INTO files(fid,fp,name_enc,size,sha,mime,kind,path,dir,created_at)
VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
```

所以数据库里的数据一直是正确的，只有 `file_info` 这一处 SELECT 的顺序反了。这也解释了为什么问题看起来「间歇性、难以复现」——数据没坏，坏的是每次读取都失败。

## 3. 修复

只改一处，把 SELECT 列顺序与读取顺序对齐：

```diff
- "SELECT name_enc,size,sha,mime,kind,path,dir FROM files WHERE fid=?1"
+ "SELECT name_enc,size,sha,mime,kind,dir,path FROM files WHERE fid=?1"
```

见 [`store.rs:468`](../src-tauri/src/store.rs#L468)。

由于只是读取端顺序错误、落库数据本就正确，**无需任何数据迁移**，重启应用即生效。

## 4. 验证

- `cargo check` 通过（Rust 侧编译无误）。
- 逻辑推演：修正后第 5 列读到 `dir`（整数）→ `as i32` 成功；第 6 列读到 `path`（文本）→ 返回 `String` 成功，`file_info` 正常返回 `Some(...)`，`build_msg_view` 恢复填充 `fpath`/`fname`/`fsize`/`mime`，图片与文件卡片恢复正常。

## 5. 附带修复

同一根因还间接影响另外两处逻辑，本次一并恢复：

1. **转发文件**（[`commands.rs`](../src-tauri/src/commands.rs) `forward_messages`）：之前 `file_info` 报错导致非文字消息转发永远走「文件不存在」分支。
2. **删除消息时的磁盘清理**（[`store.rs`](../src-tauri/src/store.rs) `delete_messages`）：之前读不到 `path`，删消息后文件仍残留在 `files/` 目录。

## 6. 教训与回归预防

1. **SELECT 列顺序是隐式契约**。用位置索引（`get::<T>(i)`）读取查询结果时，列顺序一旦与解构顺序错位，编译器无法发现，只能在运行时以「类型不匹配」的形式暴露。约定：
   - 读列时优先使用**列名**（`row.get::<_, T>("name")`）而不是位置索引，或
   - 保证 SELECT 列顺序与解构元组顺序**逐位一致**，并写注释标明。
2. **不要静默吞掉 `file_info` 的错误**。`build_msg_view` 中的 `.ok().flatten()` 把关键读取错误降级为「字段缺失」，掩盖了真实故障。对这类核心路径，至少记录一条 `tracing::error!` 日志，便于下次快速定位。
3. **`files` 表新增 `kind` 列时未同步所有读取点**。本次 `kind` 是在设计之后追加的列，SELECT 时插在了 `path` 前面却未同步下游解构，属于典型的「加列未回归」缺陷。建议后续加列为该表读取补一个最小单元测试（构造临时 DB → `upsert_file_meta` → `file_info` → 断言各字段逐位相等）。
