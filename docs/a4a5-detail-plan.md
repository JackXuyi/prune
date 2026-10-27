# Prune · A4/A5 明细实现方案

> 2026-10-27 · 上游：`prune-feature-spec-v2.md` 模块 A（A4 项目依赖活跃度 / A5 模拟器·Playwright）；
> `m1-disk-plan.md` D-K4（当时判定「M1 不做」，A7 增量已把体积/mtime 变成扫描副产品，明细几乎零成本）。
> 前置：A7 增量扫描已落地（`docs/a7-incremental-plan.md`）——node_modules 与首层条目的 mtime、
> 字节都在扫描路径上，明细只是**把已有数据多暴露一层**。

---

## 0. 结论先行

| # | 结论 |
|---|---|
| 1 | 明细 = `CacheGroup.details: Vec<DetailItem>`（`name / path / bytes / age_days / note`，serde default 空），本轮新增四组：**node_modules（A4）/ simulator / playwright / cypress（A5）** |
| 2 | `age_days` 来自扫描时已取得的 mtime（纳秒）：node_modules 用增量快照的目录 mtime，A5 组用首层指纹 mtime——**增量扫描同样产出明细** |
| 3 | A5 设备明细的可读化：`device.plist` 是 XML，抽 `name` + `runtime` 得「iPad mini (6th generation) · iOS 17.5」，解析失败回退 UDID；Playwright/Cypress 用首层目录名 |
| 4 | 清理通道（护栏扩展 D-A45-2）：**有明细的 L3 组一律逐项进 preview**（整组 root 不再作为待删项）；`execute` 护栏 1 扩展为「静态白名单 ∨ 报告派生白名单」，后者带两条硬护栏：canonicalize 后必须在 home 内 + **目录名必须是 `node_modules`**——即便报告被篡改，能删的也只有名为 node_modules 的目录 |
| 5 | node_modules 组 `cleanable` 保持 `false`（root 是 home 的聚合项永远不能整项删），可清理性下沉到明细项；前端 >180 天标灰（功能方案口径），明细按体积降序、UI 只渲染 Top 20 |

---

## 1. 数据模型与来源

```rust
pub struct DetailItem {
    pub name: String,      // 展示名（相对 home 的项目路径 / 设备名 / chromium-1155）
    pub path: String,      // 待清理路径（报告派生白名单的成员）
    pub bytes: u64,
    pub age_days: u64,     // 距上次改动（mtime）
    pub note: Option<String>, // Simulator：机型 · 运行时
}
```

| 组 | 来源 | name | note |
|---|---|---|---|
| node-modules | 增量快照 `node_modules` 表（path → mtime, bytes） | 相对 home 的路径 | — |
| simulator | `resolve_group` 首层指纹 + device.plist | 设备名（回退 UDID） | `iOS 17.5` 等 |
| playwright / cypress | 首层指纹 | 目录名 | — |

体积口径不变（物理块、跳过 symlink）——pnpm 式 node_modules 里的 store 符号链接不计入，整项移废纸篓也不波及 store 本体。

---

## 2. 清理通道与护栏

preview 逻辑变更（`reclaim::preview`）：

```
L3 且 details 非空 → 逐项生成 ReclaimItem（level=L3，needs_confirm=true），跳过整项分支
L3 且无明细       → 原整项行为（xcode-archives 等不变）
!cleanable 且无明细 → 原「只做统计」说明
```

execute 护栏 1 扩展：

```
路径放行 = whitelist::is_whitelisted（原护栏，不动）
        ∨ is_report_detail_path：
            canonicalize 成功
            ∧ 在 home 内
            ∧ file_name == "node_modules"   ← 硬护栏
            ∧ 精确出现在本次报告明细表中（canonicalize 后相等）
```

其余三条护栏不变：L3 需确认、执行时重算体积走熔断、废纸篓。`execute` 新增 `allowed: &[String]`
参数由调用方从当前报告明细表组装；CLI v1 只做 L1，传空表。

---

## 3. UI

- 分组行：有明细的组可展开，按体积降序列 Top 20（每项：体积 · 「N 天未改动」 · 名称），**>180 天标灰**（`ink-faint`）；
  「另有 N 项」提示。node-modules 组从「只统计」变为「预览」（走逐项 dry-run + L3 确认框）。
- 术语硬约束沿用：只说「未改动 / 移入废纸篓」，不出现「清理垃圾/加速」。

## 4. 验收标准

1. node_modules / simulator / playwright / cypress 四组明细随全量与增量扫描生成，age 与体积同口径
2. device.plist 解析出「设备名 · 运行时」，损坏 plist 回退 UDID 不报错
3. preview：有明细的 L3 组逐项列出且 needs_confirm=true；无明细 L3 组维持整项行为
4. execute：node_modules 路径无报告授权 → 拒绝；有授权但 file_name ≠ node_modules → 拒绝；家目录/白名单外照旧拒绝
5. 真机 dry-run：node_modules 明细逐项列出（>180 天标灰数据正确），simulator 明细含设备名
6. 门禁全绿（cargo test --workspace / cargo check / pnpm build）；CLI/MCP 输出自动携带 details（serde）

## 5. 任务分解

| ID | 任务 | 产出 |
|---|---|---|
| T-A45-1 | disk：DetailItem + 四组明细构建 + plist 解析 + 单测 | 明细随扫描产出 |
| T-A45-2 | reclaim：逐项 preview + 报告派生白名单 + 单测 | L3 逐项清理闭环 |
| T-A45-3 | commands.rs / CLI / 前端接线 | UI 可见可清 |
| T-A45-4 | 真机验收 + 验收记录 | A4/A5 关闭 |
