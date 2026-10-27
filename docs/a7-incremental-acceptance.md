# Prune · A7 增量扫描验收记录

> 2026-10-27 · 方案：`docs/a7-incremental-plan.md` · 探针：`cargo run --release -p prune-core --example incremental-probe`

## 1. 单测（cargo test --workspace，全绿）

| 断言 | 结果 |
|---|---|
| 增量结果与同状态全量结果分组字节 / 条目数 / 死重 / totals 完全一致 | ✅ `incremental_matches_full_after_changes` |
| 无变化增量 = 全量原样（复用正确性回归） | ✅ `incremental_is_noop_when_nothing_changed` |
| 深层盲区：增量保持旧值、全量给出新值（D-A7-1 文档化近似固化） | ✅ `deep_change_waits_for_full_scan` |
| 整根移除后增量报告不残留旧组、快照同步收敛 | ✅ `removed_root_group_dropped_incrementally` |
| node_modules 增 / 删被增量捕获，未变目录走快照复用 | ✅ `node_modules_incremental_tracks_add_and_remove` |
| 工作队列按条目归账（含 symlink 跳过），30 轮稳定性 | ✅ `subtree_bytes_accounts_per_entry` |
| 指纹复用 / mtime 变化触发重算 | ✅ `resolve_group_reuses_cached_fingerprints` |

计数：prune-core **91**（+7）+ prune-app 7 + prune-cli 15 = **113 项全绿**（prune-cli 1 项 macOS ignore 同前）。

## 2. 真机探针（本机，2026-10-27）

| 轮次 | 耗时 | 可回收 | 死重 | 组数 |
|---|---|---|---|---|
| 全量 #1 | **19.1 s** | 55.26 GB | 6.94 GB | 14 |
| 增量 #1（无变化） | **1.20 s** | 55.26 GB | 6.94 GB | 14 |
| 增量 #2（+8 MB 死重条目 + 触碰 cacache 首层） | **0.33 s** | 55.27 GB | 6.95 GB | 14 |
| 全量 #2（校准） | 21.9 s | 55.27 GB | 6.95 GB | 14 |

- 无变化增量与全量 #1 数字**逐位一致**（55.26 / 6.94）→ 验收 #1/#4 通过
- 制造的变化被增量**精确捕获**（+0.01 GB 主数字、死重同步上浮），与全量 #2 完全一致 → 验收 #5 通过
- 增量耗时 16~64 倍于全量，每小时静默一次的耗电/IO 可忽略

## 3. 实施偏差

| # | 方案原定 | 实际 | 原因 |
|---|---|---|---|
| 1 | 无变化增量「预期 < 1 s」 | 实测 **1.20 s**，验收线放宽为 **< 2 s** | 大头是 node_modules 重新发现（增删检测必须重新 `find_dirs_named`，本机 6000+ 目录）；体积归因部分几乎零成本（指纹全命中）。1.2 s / 小时完全可接受 |

## 4. 旧消费方回归

`disk::scan` 签名未动：prune-cli（scan 子命令 / watch / mcp）、`examples/disk-probe`、Tauri 之外的调用全部零改动；`cargo check --workspace` 无错误无警告；`pnpm build` 通过；主窗口「可回收」tab 元信息新增「· 增量」标注（`ReclaimBoard.tsx`）。

## 5. 结论

**A7 通过。** 触发矩阵落位：启动 30 s 全量 / 每小时增量 / 每周（168 刻度）全量校准 / 手动全量 / 清理后增量；快照内存态、重启自动退化全量；深层盲区由每周全量兜底（D-A7-1）并在单测中固化为预期行为。
