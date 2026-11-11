# F9 项目健康卡（M24）方案

> 定位：把「55 个 node_modules 共 4.2 GB」这种聚合数字，拆成**按项目的一本依赖账本**。
> 只体检、不代删——清理仍走既有「项目依赖」分组的逐项确认通道。

## 1. 目标与边界

| 项 | 内容 |
|---|---|
| 目标 | 开发者视角的资源账本：哪个项目吃空间最大、哪个依赖半年没重装、哪个项目锁文件打架 |
| 数据来源 | **零新增扫描**——复用扫描已产出的 `node_modules` 聚合表（path + mtime + bytes）；每个项目额外 6 次 `stat`（4 个锁文件 + package.json + `.git/index`），55 项目 ≈ 330 次，微秒级 |
| 不做 | 不代删、不做单项目清理 API（现状：L3 明细通道一次列出全部 node_modules，逐项确认）；不读源码、不判断「项目死没死」到具体结论，只给「多久没动」的事实 |
| 红线 | 结论用「多久没重装 / 多久没动」的事实表述，不用「废弃」「可以删了」这类判决式措辞 |

## 2. 项目识别口径

- 项目根 = `node_modules` 的父目录（`find_dirs_named` 命中即停止下钻，天然不含嵌套依赖）
- 展示名 = 相对 home 的路径（`Desktop/git/prune`）；不在 home 下则回退绝对路径
- 同一项目若有多个 node_modules（罕见），按项目聚合：体积求和、**依赖年龄取最小值**（最近一次安装 → 判「僵尸」时偏保守）

## 3. 健康指标

| 指标 | 口径 | 阈值 |
|---|---|---|
| `depsBytes` | 该项目 node_modules 物理块合计 | — |
| `depsAgeDays` | node_modules 目录 mtime 距今天数（安装/删除都会刷新） | — |
| `activityAgeDays` | `package.json` / 锁文件 / `.git/index` 里**最新**的 mtime 距今天数；都没有则 null | — |
| `lockfiles` | 同时存在的锁文件（package-lock.json / yarn.lock / pnpm-lock.yaml / bun.lockb） | ≥2 → 多管理器并存 |

### 结论（verdict，按优先级取一）

| verdict | 条件 | 文案 |
|---|---|---|
| `zombie` | `depsAgeDays ≥ 60` **且** `activityAgeDays ≥ 60`（或 null） | 僵尸依赖：项目也停了 |
| `heavy` | `depsBytes ≥ 1.5 GiB` | 依赖偏胖 |
| `stale` | `depsAgeDays ≥ 60`，项目还在动 | 依赖久未重装 |
| `healthy` | 其余 | 健康 |

`multiManager` 是独立布尔标记（可与任何 verdict 并存）——锁文件打架和体积无关。

阈值常量：`PROJECT_STALE_DAYS = 60`、`PROJECT_HEAVY_BYTES = 1.5 GiB`。

## 4. 数据流

```
scan_with()
  └─ nm_map: {node_modules 绝对路径 → (mtime_nanos, bytes)}
       └─ project::build_projects(&nm_map, home, now_nanos)
            ├─ 按父目录聚合体积 / 取最小 age
            ├─ 每项目 stat 标记文件 → activityAgeDays + lockfiles
            └─ Vec<ProjectHealth>（按 depsBytes 降序）
                 └─ DiskReport.projects（serde default，旧报告反序列化为空）
```

前端 `ReclaimBoard` 在「全部可回收项」之前插入 `ProjectHealthCard`，渲染 Top 6 + 汇总行；点击行 → Finder 定位项目目录（复用 `revealPath`）。

## 5. UI 形态

```
项目体检                                    6 个项目 · 依赖 9.1 GB
2 个僵尸依赖（≥60 天没重装）· 1 个项目锁文件打架

  Desktop/git/prune        3.1 GB   [偏胖]          依赖 3 天前装过 · 项目今天动过
  Documents/approval       1.8 GB   [僵尸] [多锁]    依赖 210 天没装 · 项目 210 天没动
  code/legacy              1.2 GB   [僵尸]          依赖 420 天没装 · 项目 420 天没动
```

- 只显示「有话说」的项目吗？不——**全列 Top 6**，健康项目也列（账本的意义是完整，不是只报坏消息）
- 空态：没有 node_modules 时整卡不渲染
- 每个数据点都可追溯到一次 stat，不做推断性描述（不显示「预计可省 X」）

## 6. 里程碑

| 阶段 | 内容 |
|---|---|
| T1 | `disk/project.rs`：聚合 + 结论判定 + 单测（聚合/僵尸/偏胖/多锁/排序/相对路径） |
| T2 | `DiskReport.projects` 接线（scan_with + reclaim 测试构造点同步，serde default 兼容旧缓存） |
| T3 | 前端：`types/disk.ts` + `ProjectHealthCard` + ReclaimBoard 挂载 + dev fixture |
| T4 | 全量回归（core / app / tsc / fmt）+ README & CHANGELOG |

## 7. 风险与对策

| 风险 | 对策 |
|---|---|
| mtime 被 `git checkout` / rsync 改写成「很新」 | 结论只说「多久没动」，不说「项目废弃」；僵尸判定需依赖年龄与项目活动**同时**超阈值 |
| 增量扫描复用体积快照 → 项目账本滞后 | mtime 每轮都重算，体积沿用旧快照与明细表同口径（同一目录不会出现两个数字） |
| 项目多时报告变大 | 每项目一条（~55 条 × 约 200 B ≈ 11 KB），可接受；不设上限以免账本不完整 |
| Windows 无 `.git/index`？ | `activityAgeDays` 为 null 时退化为仅看依赖年龄，不误判健康 |
