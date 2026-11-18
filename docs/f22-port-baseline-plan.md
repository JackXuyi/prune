# F22 端口可信基线与漂移（Port Baseline Drift）

> 状态：方案（未实施）。P1，注意与路线图「冻结第五类事件」裁决的对齐方式见 §4.4。
> 竞品依据：PortAura（2026-10）验证了「记住正常状态，异常才响」的需求，
> 但它刻意不做自动基线；Prune 的 scan_log + 冷却机制可复用，且基线可持久化做得更诚实。

## 一、背景

- 用户高频问题：「这个端口昨天还没有，今天哪来的？」——当前 Prune 只答「现在谁占着」，
  没有跨时间视角。
- 已有地基：30 s 端口轮（`port_sampler.rs`）、PortGuardMonitor 关注层事件（M19）、
  lan_history 的首扫基线模式（`first_scan_baseline`，先例）、SQLite `scan_log` 表。
- 基线的价值在**持久化**：重启后仍知道「哪些端口是老住户」。这与 PortGuard 的
  内存态监视器不同，需要落库。

## 二、成功标准

1. `prune port --drift` 输出相对基线的新增/消失端口（含进程与绑定面）。
2. 基线跨重启持久；用户可显式重置（`prune port --drift --rebase`）。
3. 端口页对「不在基线的新监听」给行内徽标（不进灵动岛，见 §4.4）。
4. 全程零新增采集（复用 30 s 轮快照）。

## 三、不做什么

- **不做自动「清理漂移」**——漂移是事实不是错误；dev server 每天换端口是常态。
- **不做 disappearance 事件**（基线有现在无 → 提报）——「服务停了」的噪音远大于信息，
  只进 CLI 统计不进 UI 徽标。
- **本期不做岛事件**——路线图已裁决冻结第五类事件（F18 是 opt-in 例外），第六类
  事件在 M-B 访谈验证前不立项（§4.4）。
- **不做基线学习期自适应**（自动把运行 X 天的端口吸进基线）——会静默漂移基线本身，
  违背「可信」二字；重置必须显式。

## 四、方案

### 4.1 基线模型（prune-core/src/net/baseline.rs 纯逻辑 + db.rs 落库）

```sql
CREATE TABLE IF NOT EXISTS port_baseline (
    port       INTEGER NOT NULL,
    proto      TEXT    NOT NULL,
    comm       TEXT    NOT NULL,     -- 基线键含进程名：换程序占同端口 = 漂移
    first_seen INTEGER NOT NULL,
    PRIMARY KEY (port, proto, comm)
);
```

- **基线键 = (port, proto, comm)**：不含 pid/绑定面——node 重启 pid 必变，若入键则
  每次重启都是漂移，基线废掉。绑定面变化值得单独提示（「3000 从 loopback 变成
  0.0.0.0 了」比「3000 新出现」更重要），作为漂移明细的一类（§4.2），但不入键。
- **判定纯函数** `diff(baseline, snapshot) -> DriftReport { added, removed, exposure_changed }`，
  落库读写放 db.rs（同 Store 其它表模式）。

### 4.2 三类漂移（信息量排序）

1. **暴露面变宽**（Loopback → Interface/Any）：安全相关度最高，徽标 warn。
2. **新监听**（基线无此 (port, proto, comm)）：徽标 accent。
3. **消失**（仅 CLI 统计行）：不进 UI。

### 4.3 消费面

- **CLI**：`prune port --drift [--json]`（port 子命令加 flag，不开新子命令，契约同变更）；
  `--rebase` 以当前快照重建基线并打印重建规模。首次使用基线为空 → 输出
  「首轮：当前 N 个监听已记为基线」而不是谎报 N 个漂移。
- **MCP**：`port_conflicts` 旁新增字段不合适（工具面冻结敏感），本期 MCP **不做**，
  agent 可走 `prune port --drift` 的 JSON（MCP 用户可用 Bash 跑 CLI）——工具面
  扩张的成本（契约+README+三方对账）与收益不匹配，记为显式取舍。
- **前端**：端口行徽标「新」（accent）/「面变宽」（warn），判据由 Rust 在快照里算好
  随行下发（`drift: Option<DriftKind>`），前端不持基线。首次基线建立后 24 h 内
  新监听标「新」，避免首装满屏徽标。

### 4.4 与「冻结第五类事件」裁决的对齐（关键）

- 本方案**只做 CLI + 行内徽标，不做灵动岛事件**。裁决冻结的是「新增事件类别」，
  行内徽标是查询态呈现（用户主动看端口页才可见），不构成打扰型事件。
- 若 M-B 访谈证实用户想要推送，走 F18 的处置模式（opt-in 默认关 + 「实验功能」标注）
  单独立项，不在本方案内埋伏笔。

### 4.5 基线维护

- 落库时机：每轮端口采样后 append 差异（新监听 upsert first_seen 不变；消失**不删行**，
  留 `last_seen` 供「消失」统计与快速重连识别；表膨胀由 30 天 prune 兜底）。
- 重置：`--rebase` 清表重建；设置页不做重置入口（CLI 足够，避免 UI 权责扩散）。

## 五、里程碑

| 里程碑 | 内容 | 验证 |
|---|---|---|
| M-A | db 表 + baseline diff 纯函数 + 单测（含 comm 换名/绑定面变宽/首扫基线） | cargo test |
| M-B | 采样轮接基线 diff + 快照下发 `drift` 字段（serde 全链路） | cargo check + 真机 |
| M-C | CLI `--drift/--rebase/--json` + cli-reference/契约同变更 | cargo test + check:docs |
| M-D | 前端 types/devFixture + 行内徽标 + 24 h 新鲜期 | tsc + 真机 |
| M-E | 门禁全套 | check:* 全绿 |

## 六、风险与对策

- **基线被临时端口污染**（CI 跑一轮留下一堆一次性端口）：`--rebase` 显式重置 +
  文档写清；不做自动老化（§3）。
- **root 盲区**：无权限归属的监听进不了基线，Coverage 口径随快照天然携带，
  drift 报告沿用 coverage 说明，不谎称全覆盖。
- **锁竞争**：基线写在采样轮线程（单写者），读在 CLI（打开同一 db）——Store 内部
  Mutex 模式已验证，WAL 模式下读写并见。
- **表膨胀**：消失行留痕 + `prune_before(30d)` 兜底，与 mem_process 同模式。

## 七、验收清单

- [ ] 单测：首扫基线不报漂移；comm 换名 = 新增；pid 重生 ≠ 漂移；Loopback→Any = 变宽
- [ ] 真机：起一个新 dev server → 30 s 内端口行出「新」徽标；重基线后徽标消失
- [ ] CLI：`--drift` 三类输出；`--rebase` 后 added 归零；首装首跑走「首轮基线」文案
- [ ] 跨重启：kill App 重开，基线仍有效（重启不产生满屏「新」）
- [ ] 契约：cli-reference flag 对账、`--json` 键序进冻结快照
- [ ] 门禁全套绿；无岛事件路径（方案红线，grep 可证）

## 八、实现记录（2026-11-18）

- 落点：`net/baseline.rs`（DriftKind/BaselineRow/diff/merge 纯函数，5 单测）/ db `port_baseline` 表 + `port_baseline_all/replace`（整表事务替换，单写者语义）/ 采样轮接线（port_round 内 try_state Arc<Store>，判定随轮下发 `entry.drift`，失败不阻塞快照）/ CLI `--drift [--rebase]` + 契约 port_drift 键 / 前端 DriftKind + 行内徽标 + 双语文案。
- 真机踩坑并修复（比方案多出的两条纪律，已写进代码注释）：
  1. **seed 行「天生老住户」**：merge 在基线为空/rebase 时 first_seen = now - 24h——否则下一轮 diff 把整张基线标成「新」（首装满屏徽标的老坑）；
  2. **新鲜期判据用严格小于**：seed 行恰好差 24h 整点，`<=` 会误命中。
- 与方案偏差：diff 对多持有者（冲突）端口只按 primary 判定、冲突归 F4 保镖不重复吓人；rebase 文案行数随快照如实输出。
- 真机验证：`port --drift` 首轮建基线 → rebase → 二轮「与基线一致」。baseline 键在 CLI 与 GUI 采样轮共享同一张表。
- 门禁：core 532 / cli 59 全绿（契约含 port_drift 键）；tsc / check:i18n / check:docs 通过。
