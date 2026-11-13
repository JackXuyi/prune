# 归档卡死诊断与优化方案（M30 / F14）

日期：2026-11-13　版本：0.9.0 诊断 → **0.9.1 已实施**

---

## 0. 结论先行

**归档命令是同步 Tauri command，跑在 macOS 主线程上，把 900 MB 级 `node_modules` 的 50–90 秒压缩工作整个压在事件循环里。** 不是内存泄漏、不是死锁、不是废纸篓卡住——是**执行模型选错**：整个仓库只有这 6 条命令是同步的，也恰好只有它们做分钟级文件与 CPU 工作。

修法明确：改成 `async fn + spawn_blocking`（仓库里 `reclaim_execute` / `app_disk` 早就是这个写法），补进度与取消，压缩参数从上往下调一档。**0.9.1 已全部实施**，效果见 §7。

---

## 1. 证据链

### 1.1 代码层：6 条命令全是同步

`src-tauri/src/commands.rs:659-718` 全部是 `pub fn`，没有 `async`：

```rust
#[tauri::command]
pub fn archive_create(project_root: String, confirmed: bool) -> Result<ArchiveEntry, String>
```

Tauri v2 官方文档原文（[Calling Rust from the Frontend](https://v2.tauri.app/develop/calling-rust/)）：

> Asynchronous commands are preferred in Tauri to perform heavy work in a manner that doesn't result in UI freezes or slowdowns. Async commands are executed on a separate async task using `async_runtime::spawn`. **Commands without the *async* keyword are executed on the main thread** unless defined with `#[tauri::command(async)]`.

主线程 = macOS 的 NSApplication 事件循环。它被占住 → 不重绘、不响应点击、WindowServer 判定无响应 → **转彩虹球**。灵动岛的动画循环虽然跑在独立 async task 上，但**画**要主线程，所以视觉上一起冻住。

### 1.2 对照层：仓库里其它长任务都规避了

| 命令 | 写法 | 工作量 |
|---|---|---|
| `reclaim_execute` | `async` + `spawn_blocking` | 秒级废纸篓 |
| `reclaim_children` | `async` + `spawn_blocking` | 秒级目录遍历 |
| `app_disk` / `app_bundles` / `app_icons` | `async` + `spawn_blocking` | 秒级 |
| `disk_scan` | `async` + 后台线程 + 事件 | 15 秒全盘扫描 |
| **`archive_plan` / `archive_create` / `archive_restore` / `archive_delete`** | **同步 `pub fn`** | **50–90 秒** |

即：**长任务不出主线程的约定本来存在，M30 是唯一破例的一处。**

### 1.3 数据层：本机实测耗时账单

样本：`~/Desktop/git/wcs-homepage/node_modules`，**922 MB / 36,734 条目**（本机三个项目里最大的一个，属正常量级；Electron 类项目常到 2–3 GB / 20 万条目）。

| 阶段 | 实现位置 | 实测耗时 | 备注 |
|---|---|---|---|
| ① 统计 `walk_counts` | `archive.rs:308` | **0.74 s** | 全量 `symlink_metadata`，单线程栈遍历 |
| ② 打包 `write_archive` | `archive.rs:333` | **46.3 s** | `tar czf` 同参数；922 M → 227 M |
| ③ 校验 `verify_archive` | `archive.rs:349` | **1.30 s** | 解压全量重读 |
| ④ 删原件 `trash_delete` | `archive.rs:153` | 数秒～数十秒 | 36k 文件走 NSFileManager，未单独实测 |
| **合计** | | **≥ 50 s，全在主线程** | |

两点会让实际值比这更差：

1. **flate2 默认后端是 `miniz_oxide`（纯 Rust），比系统 C zlib 慢约 1.5–2 倍**。上表 ② 用的是系统 `gzip`，应用内实际可能 **60–90 秒**（此项为推断，需在应用内实测确认）。
2. 冷缓存（刚 clone、刚 install）首次读取要真读磁盘，比热缓存再慢一截。

对照实测：**压缩级别降到 1，同一份数据 22.2 s / 268 M**——快 2.1 倍，体积只大 18%。这是最划算的一档调整。

### 1.4 交互层：为什么用户感知是「卡死」而不是「处理中」

`ProjectHealthCard.tsx:145-155` 的 `beginArchive` **完全没有 pending 态**：

```tsx
const beginArchive = async (p: ProjectHealth) => {
  try {
    setDialog({ kind: "archive", plan: await archivePlan(p.root) });   // 无 setBusy
  } catch (e) { setToast(String(e)); }
};
```

而 `runDialog` 里的 `setBusy(true)`（第 170 行）虽然把按钮文案切成「处理中…」，但**这次 React 重绘本身需要主线程**——主线程已经被 `archive_create` 占死了，所以连「处理中…」都画不出来。

净效果：点确认 → 界面定格在旧画面 → 转彩虹球 → 用户只能等，或者强杀。

### 1.5 附带缺陷：强杀会留下孤儿归档

`write_archive` 直接写最终路径（`archive.rs:334`），账本条目要等**校验通过后**才写（第 220 行）。这中间有 46–90 秒的窗口：

- 用户强杀 / 系统重启 → 盘上留下一个半截 `.tar.gz`
- 账本里没有它 → **UI 看不见、删不掉、也不参与清理**，纯占空间
- 且 `id` 含时间戳，下次归档不会撞名，孤儿会一直躺着

这个窗口在改成异步后依然存在（异步只是不卡 UI，中断风险不变），所以要在 P0 一并修掉：**先写 `.part`，成功后再 rename 到最终路径**——rename 是原子的，孤儿最多是 `.part`，启动时扫一遍删掉即可。

---

## 2. 根因分级

| 级别 | 根因 | 影响 | 位置 |
|---|---|---|---|
| **R1（决定性）** | 命令同步执行，占用主线程 | 全程 UI 无响应、彩虹球、系统可能弹「应用无响应」 | `commands.rs:659-718` |
| **R2（放大痛感）** | 无进度、无 pending 态、不可取消 | 用户无法区分「在干活」和「卡死了」，只能强杀 | `ProjectHealthCard.tsx:145`、`archive.rs` 全程无回调 |
| **R3（量级）** | gzip level 6 + `miniz_oxide` 后端 | 46 s 里绝大部分是这 22 s 与后端差异 | `archive.rs:335` |
| **R4（数据完整性）** | 直写最终路径、账本后写 | 强杀留孤儿归档 | `archive.rs:334`、`220` |
| R5（次要） | 无并发闸门 | 连点两次并行压缩，CPU 打满、归档区争抢 | `commands.rs` |

---

## 3. 优化方案

### P0 —— 止血：不再卡死（约 80 行，当天可完）

| # | 改动 | 文件 | 要点 |
|---|---|---|---|
| 1 | 4 条重命令改 `async fn` + `spawn_blocking` | `commands.rs` | 照抄 `reclaim_children` 的写法；`archive_list` 只读小 JSON，保留同步无妨 |
| 2 | `beginArchive` / `beginRestore` 加 pending 态 | `ProjectHealthCard.tsx` | 按钮禁用 + 「统计中…」；`archivePlan` 虽只 0.7 s，但冷缓存下会有可感延迟 |
| 3 | 归档落盘改 `.part` + `rename` | `archive.rs` | 取消/强杀最多留 `.part`；启动时清理过期 `.part` |

关键：`spawn_blocking` 必须 `.await` 回来再返回，**不能用 `tauri::async_runtime::spawn` 后立即返回**——那样前端拿不到结果。

改动后主线程占用从「分钟级」降到「微秒级」，卡死立即消失；此时 46 s 的活还在，但界面能画进度、能响应。

### P1 —— 可感知 + 快一倍（约 150 行）

| # | 改动 | 要点 |
|---|---|---|
| 4 | `archive://progress` 事件 | payload `{ stage, doneBytes, totalBytes, files }`；stage = `plan` / `pack` / `verify` / `trash`。打包循环里按「≥200 ms 或 ≥32 MB」节流 emit，避免 200k 条目淹掉 IPC |
| 5 | `archive_cancel` 命令 | `Arc<AtomicBool>` 令牌，打包循环每 N 条目检查一次；取消 → 丢弃 builder → 删 `.part` → 返回「已取消」。前端进度条旁给取消按钮 |
| 6 | 压缩参数 | `Compression::fast()`（level 1）：实测 **46.3 s → 22.2 s**，体积 227 M → 268 M（+18%）。归档的价值是「可还原」不是「最小体积」，这个交换划算 |
| 7 | 换 zlib 后端 | `flate2 = { version = "1", default-features = false, features = ["zlib-rs"] }`——纯 Rust 的 zlib-rs 通常比 miniz_oxide 快 1.5–2 倍。**需实测确认：先本地 benchmark，无收益就退回**（不当成既定结论写进 CHANGELOG） |
| 8 | 并发闸门 | 静态 `AtomicBool`，已有归档在跑时立即返回「上一个归档还在进行中，请稍候」——比让两个任务争 CPU 更友好 |

第 4 项的落地前提：`write_archive` 要从 `builder.append_dir_all()` 改成自己遍历 + `append_file()` 才能按条目报进度。遍历本身 0.74 s，可接受（也让 `walk_counts` 与打包共用一份遍历逻辑）。

### P2 —— 打磨（可选）

- 进度条上显示「已处理 340 MB / 922 MB · 预计还需 12 秒」
- `trash` 阶段也报进度（大目录这一步可能数十秒，不能变成新的「静默黑洞」）
- 恢复流程的 `unpack` 同样接进度（解压 1.3 s 无需进度，但覆盖删除可能慢）
- 归档完成后自动重扫项目体检，省一次手动刷新

---

## 4. 明确不动的部分

优化不能拿安全换速度：

- **`verify_archive` 全量解压校验必须保留**（1.3 s，是「校验通过才删原件」的唯一凭据）
- **`walk_counts` 与打包各自遍历一次**，不复用前端传来的数字——体积基线必须后端自己算，否则一份伪造的 plan 就能绕过「校验通过才删」的护栏
- **删除一律走废纸篓**，不因为要快就换成 `remove_dir_all`
- **`confirmed` 门控与语义边界（只归档 node_modules）不动**

---

## 5. 验收标准

| 项 | 判据 |
|---|---|
| 主线程不阻塞 | 归档全程灵动岛动画连续、窗口可拖动、菜单可打开；主线程单次占用 < 50 ms |
| 反馈及时 | 点「执行归档」后 < 100 ms 出现进度条与阶段文案，之后每 200 ms 至少刷新一次 |
| 可取消 | 进度条旁取消按钮生效；取消后盘上无 `.part`、账本无条目、原件完好 |
| 强杀安全 | `kill -9` 后只可能残留 `.part`，重启应用自动清理；原件不丢 |
| 性能目标 | 922 M / 36k 条目：总时长 ≤ 20 s（当前 ≥ 50 s）；2–3 GB 项目 ≤ 60 s |
| 回归 | `cargo test --workspace` 全绿；`archive.rs` 单测覆盖「取消后无 .part」「.part 残留不影响 list」两条新路径 |

---

## 6. 建议实施顺序

1. **P0（1）+（2）**：异步化 + pending 态 → 立刻不再卡死，可单独发一版
2. **P0（3）**：`.part` 原子发布 → 补数据完整性
3. **P1（6）**：压缩级别 → 时长腰斩
4. **P1（4）（5）（8）**：进度 / 取消 / 闸门 → 体验完整
5. **P1（7）**：后端 benchmark，有收益再上

前 3 步是「不能再等」的，第 4 步起是体验问题。

---

## 7. 实施记录（0.9.1）

### 7.1 真机复测（817 MB / 30,984 个文件的 `node_modules`，release 构建）

| 阶段 | 0.9.1 实测 | 说明 |
|---|---|---|
| `plan`（统计） | **426 ms** | 遍历 3 万条目 |
| `create`（打包+校验+删原件） | **11.1 s** | 归档 291 MB |
| `restore`（解压+校验） | **9.5 s** | 写回 3 万个文件 |

同口径下 0.9.0 仅压缩一步就要 46 s（系统 gzip level 6），应用内 `miniz_oxide` 更慢，整体 ≥ 50 s 且全程锁死主线程。**现在 11 s 且界面全程可交互**，验收目标（≤ 20 s）达成。

### 7.2 逐项落地

| 方案项 | 落地方式 |
|---|---|
| P0-1 异步化 | 5 条命令改 `async fn + spawn_blocking`；`archive_list` 只读小 JSON 保留同步 |
| P0-2 反馈 | 前端 `planning` 态（统计中禁用防连点）+ 对话框进度条 + 阶段文案 + ETA |
| P0-3 原子发布 | `.part` → 校验 → `rename`；`cleanup_stale_parts` 清 >10 min 的陈旧半成品 |
| P1-4 进度事件 | `archive://progress`，阶段切换 / 200 ms / 跑满三种触发；契约测试锁字段名 |
| P1-5 取消 | `archive_cancel` + 静态 `AtomicBool`；取消后原件不动、无 `.part`、不记账 |
| P1-6 压缩级别 | `Compression::fast()`（level 1） |
| P1-8 并发闸门 | 静态 `AtomicBool` compare_exchange + RAII 守卫（panic 也会放行） |

### 7.3 实测后决定「不做」

**flate2 换 zlib-rs 后端**：同一份数据对比——打包 **12.1 s vs 11.1 s（更慢 9%）**、归档 **313 MB vs 291 MB（大 7.5%）**、解压 6.4 s vs 9.5 s（更快）。主路径是打包，且体积变大，**不换**；把这条实测写进 CHANGELOG，避免以后有人凭直觉再改一次。

### 7.4 过程中发现的额外缺陷（已修）

tar 0.4.46 的 `Builder::append_link` **不会替调用方设置 `entry_type`**（老版本会），只写路径与 linkname。于是自建的打包循环里符号链接被写成 `size=0` 的**普通文件**——pnpm 的 `node_modules` 恢复出来是坏的（`.pnpm` 链接全变成空文件）。修法：显式 `header.set_entry_type(EntryType::Symlink)`，并加「pnpm 链接目标逐字还原 + 穿透读内容」测试兜住。这个 bug 是重写打包循环时引入的，靠新增的 pnpm 用例当场发现——**若不写这条测试，0.9.1 会带着「pnpm 用户恢复即损坏」上线**。
