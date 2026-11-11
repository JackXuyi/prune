# F2 年度浪费报告（剪除成绩单）方案 · M21

> Wrapped 式「你剪掉了多少」。数据全部来自用户**亲手执行**的清理动作，本地 SQLite 落库
> （`cut_log`），零网络、零埋点——与 `net_daily` / `app_hourly` 同一纪律。

## 1. 目标与边界

**做**：
- 每次清理成功后，把剪除项（label + bytes）落库
- 总览页一张「剪除成绩单」卡：累计 GB · 次数 · 相当于什么 · 受害者 Top 3
- 空态引导：没剪过 → 「去存储完成第一次清理」

**不做（本期）**：
- 分享卡片图导出（T5 后置）
- 年度切换动画 / Wrapped 多页翻页
- swap 受害者排行榜（无落库数据源，等泄漏档案有历史再说）

## 2. 数据模型（prune-core/src/db.rs）

```sql
CREATE TABLE IF NOT EXISTS cut_log (
    ts    INTEGER NOT NULL,        -- 秒级本地时间
    label TEXT    NOT NULL,        -- 分组 label（「npm 缓存」「node_modules」…）
    bytes INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cut_log_ts ON cut_log (ts);
```

新增方法：
- `cut_add(&self, items: &[(label, bytes)], now)` —— 单事务批量插入
- `cut_summary(&self) -> CutSummary { total_bytes, total_items, first_ts }` —— 全量聚合（记录稀疏，一年几百行，不需要区间参数）
- `cut_top(&self, n) -> Vec<CutTop { label, bytes, count }>` —— GROUP BY label 降序
- `cut_prune_before(&self, cutoff)` —— 保留 800 天（两年）对齐年度口径

## 3. 后端接线（src-tauri）

- **lib.rs**：`app.manage(store.clone())` —— 把 `Option<Arc<Store>>` 注册为独立 Tauri
  状态（此前只藏在 MemState/TotalNetState 里，commands 摸不到）
- **commands.rs `reclaim_execute`**：注入 `State<Option<Arc<Store>>>`；成功后把
  `result.failed` 的路径排除，对成功项逐条 `cut_add`（单条失败不影响其余记录）
- **commands.rs `cut_report`（新）**：返回 `CutReport { total_bytes, total_items, first_ts, top }`，
  store 不可用时返回全 0（前端空态，不是报错）

## 4. 前端（总览页）

- **ipc.ts**：`cutReport()` + 类型
- **StatsBoard**：六张域卡之后追加整行「剪除成绩单」卡：
  - 主数字：累计剪掉 X GB（CountUp 复用）
  - 副行：N 次清理 · 首次 YYYY-MM-DD · 相当于 M 部高清电影（4 GB/部；总量 < 4 GB 时换算成照片 3 MB/张）
  - Top 3：label + bytes，一行一条
  - 空态：「还没有剪除记录 · 去存储完成第一次清理」（点击钻到 disk tab）

## 5. 里程碑

| # | 内容 | 验证 |
|---|------|------|
| T1 | db.rs `cut_log` 表 + 4 方法 + 单测（累加/聚合/Top N/保留期/重开存活） | cargo test -p prune-core db |
| T2 | lib.rs manage store + reclaim_execute 落库 + cut_report 命令 + 注册 | cargo check |
| T3 | ipc.ts + StatsBoard 成绩单卡（含空态与换算） | tsc --noEmit |
| T4 | 全量回归 + fmt + README/CHANGELOG | 全绿 |
| T5 | （后置）分享卡片图导出 | — |

## 6. 风险与对策

- **重复落库**：reclaim_execute 只在成功路径调用一次；若自动重扫失败不影响记录
- **bytes 口径**：按预览项 bytes 记录（执行时重算的总量仅用于日志），单条误差可忽略
- **库打不开**：cut 静默跳过（与 net_daily 同策略），报告卡显示空态而非报错
