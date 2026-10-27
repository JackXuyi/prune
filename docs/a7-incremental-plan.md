# Prune · A7 增量扫描实现方案

> 2026-10-27 · 上游：`prune-feature-spec-v2.md` 模块 A（A7）；`m1-disk-plan.md` §5.2（D-K5 当年判定「v1 不做 mtime 增量」）
> 前提已被实测改变：全量单轮 15~17.6 s（M1 验收），每周全量之间菜单栏数字完全静止；
> 且清理后重扫也要付一次全量成本。A7 的目标：**让静默刷新和清理后重扫变得便宜，同时保住数字的可信度。**

---

## 0. 结论先行

| # | 结论 |
|---|---|
| 1 | 增量粒度 = **白名单根的首层条目**：`name + mtime(纳秒)` 为指纹，mtime 没变直接复用上次体积，变了才 `tree_size` 重算 |
| 2 | 核心原语是**按条目归账的工作队列**（`subtree_bytes`）：全量与增量共用一条路径，pnpm store 这类「单个首层条目 44 万文件」的病态树仍然吃满 12 路并发，不回退到单线程 |
| 3 | 触发矩阵（关键取舍 D-A7-2）：**静默与清理后用增量，用户盯着的场景（启动首扫 / 手动刷新 / 每周校准）用全量**——增量的深层盲区不能出现在用户主动求证的瞬间 |
| 4 | 已知盲区（D-A7-1）：内容寻址缓存（npm `_cacache` / pnpm store / cargo registry）的**深层新增文件**不改变任何浅层目录 mtime，增量扫不出来 → 每周全量校准兜底，方案里明写、不装看不见 |
| 5 | 快照只存内存（`DiskState`），应用重启即失 → 下轮自动全量，符合「首次全量」语义，零持久化风险 |

---

## 1. 触发矩阵

| 触发 | 模式 | 理由 |
|---|---|---|
| 启动后 30 s | **全量** | 应用生命周期内的「首次全量」；快照在内存里，重启后必是全量 |
| 每小时静默 | **增量** | mtime 检查 + 少量重算，预期 < 1 s；菜单栏数字保持新鲜 |
| 每周（第 168 个小时刻度） | **全量** | 校准深层盲区，对齐功能方案「每周静默一次更新」 |
| 用户手动刷新 | **全量** | 用户正在盯着数字求证，17 s 的等待换 100% 准确，值得（D-A7-2） |
| 清理完成后 | **增量** | 删除必然表现为「首层条目消失」或「整个根消失」，增量 100% 捕获 |

外加一道保险：距上次全量超过 7 天时，任何一次「增量」请求自动升级为全量（防止定时器漂移导致长期无校准）。

---

## 2. 数据结构

```rust
/// 首层条目指纹：mtime 用纳秒（APFS 精度），避免同秒写入的判定歧义
pub struct EntryFingerprint {
    pub name: String,
    pub mtime_nanos: i64,
    pub bytes: u64,
}

/// 内存态快照（不持久化）
pub struct ScanSnapshot {
    pub taken_at: i64,
    /// 组 key → 首层条目指纹表（与报告 groups 的 key 一一对应）
    pub groups: BTreeMap<String, Vec<EntryFingerprint>>,
    /// node_modules 聚合：目录绝对路径 → (mtime_nanos, bytes)
    pub node_modules: BTreeMap<String, (i64, u64)>,
}
```

`DiskReport` 新增 `incremental: bool`（serde default false，旧报告反序列化兼容），UI/CLI 可据此标注「增量」。

---

## 3. 扫描路径统一

`scan_with(home, prev: Option<&ScanSnapshot>, progress) -> (DiskReport, ScanSnapshot)`：

1. 每个 A1 / 定点根：枚举首层条目（跳过 symlink）→ 与快照 diff（同 name 且同 mtime → 复用字节；否则进重算集合）→ `subtree_bytes` 工作队列只算重算集合 → A1 组喂 `deadweight_stats`，其余组求和；
2. node_modules：`find_dirs_named` 重新发现（增删必须可见）→ 逐目录 mtime 比对快照 → 只重算变化目录；
3. `prev = None` 或快照缺该组 → 全部重算，等价旧全量语义（数字必须与旧实现一致，单测锁定）；
4. 装配报告（排序 / dup / totals 逻辑不动）+ 回写新快照。

旧的 `scan(home, progress)` 签名保留 = `scan_with(home, None, progress).0`，CLI / MCP / disk-probe / Tauri 四处消费方零改动。

### 盲区边界（必须写进验收记录，不是 TODO）

写文件只改**直接父目录**的 mtime。因此：

| 变化 | 增量能否捕获 |
|---|---|
| Yarn 新装/删除包（首层新目录/目录消失） | ✅ |
| 条目内直接子项变化（该条目 mtime 变） | ✅ |
| npm cacache 深层新增 content（父链 mtime 全不变） | ❌ 等每周全量 |
| node_modules 安装/卸载（node_modules 直接子项变化） | ✅ |
| node_modules 内深层文件变动 | ❌（罕见，接受） |

---

## 4. 验收标准

1. 增量结果与同状态全量结果的分组字节、totals **完全一致**（单测）
2. 深层盲区行为被单测固化：增量保持旧值、全量给出新值（文档化近似，不算 bug）
3. 整个根被移除（如 `~/.npm/_cacache` trash 后）→ 增量报告中该组消失，不残留旧值
4. 真机实测：增量轮耗时 **< 1 s**（全量 15~17 s 对照），主数字与全量差异在盲区允许范围内
5. 清理 Yarn 条目后增量重扫，主数字下降量 = 清理量（±同轮其他变化）
6. 全量门禁（cargo test --workspace / cargo check / pnpm build）全绿；旧 `scan()` 消费方（CLI/MCP/探针）零改动

---

## 5. 任务分解

| ID | 任务 | 产出 |
|---|---|---|
| T-A7-1 | `disk/incremental.rs`：指纹 / 快照 / diff / `subtree_bytes` + `scan_with` 重构 + 单测 | 全量=增量一致性可验证 |
| T-A7-2 | `src-tauri/scan.rs`：DiskState 扩展 + 触发矩阵落地；`commands.rs` 清理后改增量 | 增量进入调度 |
| T-A7-3 | `examples/incremental-probe` 真机计时 + 验收记录 + m1-disk-plan 回扫 | A7 关闭 |
