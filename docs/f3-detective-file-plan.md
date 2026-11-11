# F3 泄漏侦探档案（悬案卷宗）方案 · M22

> 把 B5「泄漏嫌疑」从一条警告重塑成**悬案卷宗**：嫌疑人（进程）有档案照（应用图标）、
> 有证物（生长曲线）、有结案陈词；嫌疑人离场（进程退出/重启）后未复发 → 自动归档。
> 零新权限、零新采样：判定引擎、序列库、图标通道全部复用现成件。

## 1. 措辞硬约束（继承 M3 §6.3，红线）

- 永远不说「你的 XX 泄漏了」，只能说「嫌疑 / 按曲线判断 / 统计特征」
- 不出现「释放 / 加速 / 优化」
- 「结案」≠「证明清白」：结案陈词只描述**观察到的事实**（离场、未复现），不下结论
- 卷宗叙事为辅助：证据数字（增长量 / 斜率 / R²）仍然一字不减地展示

## 2. 案件簿状态机（prune-core，纯逻辑可单测）

新文件 `crates/prune-core/src/leak/cases.rs`：

```rust
pub struct LeakCase {
    pub id: String,            // "CASE-1111-03"（MMDD + 立案序号）
    pub name: String,
    pub family: String,
    pub pid: i32,
    pub start_time: u64,
    pub first_seen: i64,       // 立案时间（Unix 秒）
    pub last_seen: i64,
    pub growth_peak: u64,      // 侦查期最大净增长
    pub growth_latest: u64,
    pub status: CaseStatus,
}
pub enum CaseStatus {
    Open,
    Closed { verdict: Verdict, closed_at: i64 },
}
pub enum Verdict {
    Cleared,   // 嫌疑人离场，此后同应用未见新嫌疑 → 归档
    Relapsed,  // 嫌疑人离场，但同应用另立新案 → 疑似复发
}
```

`CaseBook::update(items: &[LeakItem], alive: &HashSet<(i32, u64)>, now) -> ()`：
1. 命中项按 `(pid, start_time)` 匹配：已有 Open 案 → 更新 last_seen / growth；
   没有 → 立新案（编号 MMDD + 全簿递增序号）
2. 上一轮 Open、本轮 pid 不在 `alive` → 结案：同 family 本轮有**新** Open 案（不同
   pid）判 `Relapsed`，否则 `Cleared`
3. `items` 消失但进程还活着（增长退回门槛下）→ 保持 Open（侦查未完，不算结案）

归档上限 20 条（超出删最旧）；案件簿在 MemState 内存中（Prune 重启即清空——
档案本来就描述「这一段试用期」，不落库，避免过度持久化）。

## 3. 后端接线（src-tauri）

- `sampler.rs`：MemState 加 `cases: Mutex<Vec<LeakCase>>` + CaseBook；深探轮
  `recompute_leaks` 之后 `casebook.update(&report.items, merged, ts)`（merged 是
  全量进程，供 alive 判定）
- 新命令 `leak_cases(state) -> Vec<LeakCase>`（读案件簿快照）
- `island` / `tray` 不动——菜单栏 Banner 维持一行提示

## 4. 前端（LeakCard 卷宗化，memory tab）

- `LeakCard` 重塑为 `悬案卷宗` 样式（保留 evidenceLine 全部数字）：
  - 案件头：`CASE-1111-03` 等宽字 + 「侦查中」徽标 + 立案时长
  - 嫌疑人行：AppIcon（用 memStore.processes 找同 family 的 exe 拿真图标，无则字母
    徽标）+ name (family) + GC 语言标记
  - 证物曲线：`memHistory(pid, 40)` → Sparkline（tone=warn），**证据数字一字不减**
  - 点击行 → 现有 `onOpen(pid)` 下钻检视器（LeakEvidence 保持原样）
  - 判据脚注保留
- 新增「档案室」折叠区（仅在有结案时出现）：结案陈词一行一条——
  `Cleared`：嫌疑人已离场，此后同应用未见新嫌疑 · 卷宗归档
  `Relapsed`：同应用另立新案，疑似复发 · 建议留意
- 数据流：memory tab 挂载 + `snapshot.leaks` 变化（每轮深探）时 `leak_cases()` 拉取
- 菜单栏 LeakBanner、AppDetail/检视器 LeakEvidence 不动

## 5. 里程碑

| # | 内容 | 验证 |
|---|------|------|
| T1 | leak/cases.rs CaseBook + 模块挂载 + 单测（立案/续期/离场结案/复发/编号/归档上限） | cargo test -p prune-core leak |
| T2 | sampler 案件簿接线 + leak_cases 命令 + 注册 | cargo check |
| T3 | LeakCard 卷宗化 + 档案室 + 证物曲线 | tsc --noEmit |
| T4 | 全量回归 + fmt + README/CHANGELOG | 全绿 |
| T5 | （后置）卷宗导出图片、跨重启持久化 | — |

## 6. 风险与对策

- **案件簿重启清零**：接受——档案描述当前试用期，持久化收益低
- **同 family 多进程并发命中**：每 (pid, start_time) 一案，互不合并（判定单元本就是单进程）
- **图标拿不到**：AppIcon 自带字母徽标降级，不阻塞
- **文案红线**：所有新文案过一遍 §6.3，卷宗叙事只是外壳
