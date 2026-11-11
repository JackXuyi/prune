# F10 · 应用历史深度 8 天 → 30 天（M25）

> P1 竞品缺口落地：Vitals 有 30 天 per-app 历史（12h/24h/7d/30d 视图），Prune 的 D3/D5 只保留 8 天、命令 clamp 10 天、前端只有 24h/7d 档。本方案把深度拉平到 30 天，**不引入任意指标的时间序列**（README 口径红线不动）。

## 0. 现状与缺口

D3/D5 底座已通：fast 轮（10 s）按家族累加 RSS/CPU → 低频网速轮累加流量 → `AppHistoryAgg` 60 s flush 到 `app_hourly`（增量 UPSERT，重启不清零）→ `app_history(family, hours)` 命令 → 应用详情 `HistoryTrack`（24h/7d，7d 由前端 `aggregateDays` 聚合小时桶）。

| 缺口 | 现状 | 目标 |
|---|---|---|
| 保留期 | `APP_HISTORY_RETAIN_SECS` = 8 天 | 30 天 |
| 命令窗口 | `hours.clamp(1, 24*10)` | `24*30` |
| 30 天视图 | 无（小时桶 720 行全拉太重） | 天粒度聚合，≤30 行 |
| 前端 | 24h / 7d | 24h / 7d / **30d** |

## 1. 设计决策

1. **不加新表**：`app_hourly` 行数预算（本机实测 657 家族 / 2 天 1.8 万行 / 库 9.4 MB）外推 30 天 ≈ 27 万行 / ~140 MB，SQLite 无压力；维护两张表会引入「小时表与天表口径漂移」的新问题。
2. **聚合下推 SQLite**：新增 `app_daily_get(family, from_day)` —— `substr(bucket,1,10)` GROUP BY 一条 SQL（720 → 30 行），均值 Σsum/Σn 重算（不是均值的均值）、峰值 MAX、流量 SUM，与前端 `aggregateDays` 逐字段一致。
3. **缺失纪律不变**：只返回有记录的天/小时，缺失由前端画灰虚线，不补 0（补 0 = 把「Prune 没运行」说成「CPU 为 0」）。
4. **覆盖率口径**：30d 档 = 有记录的天 / 30；24h/7d 档 = 有样本的小时 / 应有小时（照旧）。
5. **浏览器走查**：`useAppHistory` 无 Tauri 环境时把假小时序列聚合成假天序列，保证 30d 档结构与空态可见。

## 2. 改动清单

| 文件 | 改动 |
|---|---|
| `crates/prune-core/src/db.rs` | `DailyRow` + `app_daily_get` + 3 单测 |
| `src-tauri/src/sampler.rs` | 保留期常量 8→30 天；`MemState::app_history_daily` |
| `src-tauri/src/commands.rs` | `app_history` clamp 30 天；新命令 `app_history_daily` |
| `src-tauri/src/lib.rs` | 注册 `app_history_daily` |
| `src/types/appHistory.ts` | `DailyBucket` |
| `src/lib/ipc.ts` | `appHistoryDaily` |
| `src/lib/useAppHistory.ts` | 并行拉天桶（31 天窗口）；返回 `daily` |
| `src/components/appdetail/HistoryTrack.tsx` | 30d 档 + 动态标题「历史走势」 |
| `CHANGELOG.md` / `README.md` | M25 记录 |

## 3. 已知取舍

- fast 轮全量家族进账（657 个家族含大量系统 agent）→ 30 天 ~140 MB 库体积。对清几十 GB 缓存的开发机可接受；若未来要压，方向是「Top N 家族全量 + 长尾家族只记峰值」，本期不做。
- 天均值在 SQL 端与前端 `aggregateDays` 的 CPU 均值口径有微差（前端对小时均值再平均，SQL 按 Σsum/Σn）——30d 档只用 SQL 端，7d 档保持前端口径，互不混用。
- 深探 footprint 序列（`mem_process` 表，7 天保留）不扩——那是泄漏判定的证据序列，与 per-app 历史是两个用途。

## 4. 验证

- `cargo test -p prune-core`：328 passed（含 3 个天聚合新测试）
- `cargo check`（src-tauri）+ `tsc --noEmit`：0 错误
- 真实库验证：对本机 `app_hourly`（18158 行）手工执行天聚合 SQL，按天分组 / 均值重算 / 峰值 MAX 全部正确
