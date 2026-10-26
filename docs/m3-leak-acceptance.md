# Prune · B5 泄漏嫌疑判定（M3）验收记录

> 2026-10-26 · 对应方案 `docs/m3-leak-plan.md`
> 全部结论来自本机实测，未做任何推算。

---

## 1. 门禁

| 项 | 结果 |
|---|---|
| `cargo test --workspace` | **91 项全绿**（prune-core 84 + prune-app 7） |
| — 其中 leak 模块新增 | 15 项 |
| — 其中 db 序列查询新增 | 3 项 |
| `cargo check -p prune-app` | 通过 |
| `pnpm build`（tsc 严格 + vite） | 通过 |

---

## 2. 单测覆盖（15 项）

| 测试 | 断言 |
|---|---|
| `steady_climb_is_flagged` | 每 5 min +30 MB 的直线 → 命中，R² > 0.99，阶梯占比 < 0.2 |
| `step_jump_is_rejected_like_real_db` | 真实库里 pid 50392 的形态（37 MB 平台 → 单点 686 MB）→ 拒绝 |
| `step_dominant_growth_is_rejected_even_with_good_fit` | **阶梯规则独立存在理由**：该形态 R² 仍 > 0.70，只有阶梯规则能挡；同量级无阶跃的曲线 → 命中 |
| `sawtooth_is_rejected` | 基准爬升叠加 ±抖动 → 拒绝 |
| `flat_is_rejected` | 平线 → 拒绝 |
| `big_process_slow_growth_is_rejected` | 3 GB 进程涨 160 MB（5.3%）→ 拒绝 |
| `insufficient_series_never_produce_items` | 只有 3 个点 / 进程 10 min 前才启动 → 不产出 item |
| `data_gap_invalidates_window` | 窗口内 20 min 断档 → 数据不足 |
| `benign_objects_are_never_flagged` | Chrome / Electron / Docker / Prune / 系统进程 / qemu → 全部良性 |
| `named_electron_apps_are_still_judged` | Cursor 进程仍参与判定（D-L2） |
| `gc_runtime_needs_double_threshold` | Node 30 min +200 MB → 数据不足；普通进程同数据 → 命中；Node 60 min +360 MB → 命中 |
| `reused_pid_series_are_judged_independently` | 同 pid 不同 start_time 各自独立判定 |
| `report_counts_and_orders_items` | 计数分类正确、items 按增长降序、家族数与合计正确 |
| `unavailable_report_says_why` | 无库 → `note` 非空，不冒充「0 个泄漏」 |
| `fit_rejects_flat_and_short` | 平线 R² 记 0（不是 1）、单点返回 0 |

db 侧 3 项：`series_splits_on_pid_reuse`（pid 复用必须切成两条序列）、`series_skips_rows_without_footprint`（RSS-only 行不进判定）、`series_filters_by_since`。

---

## 3. 真实库实测（2026-10-23 的 `prune.db`，5.96 h / 68 个时间点 / 4441 行）

```
$ cargo run --release -p prune-core --example leak-probe

rows = 4441 · 序列 = 171 条 · now = 1792748383

== 判定结果 ==
参与 95 · 命中 0 · 良性 23 · 数据不足 14 · 未达标 58
→ 0 个疑似内存泄漏（这是正确结果，不是「没在检查」）

== 净增长 > 100 MB 但未命中的进程 ==
  Cursor pid 674 · 增长 182.4 MB（1036.4→1218.8）· 数值条件未达标（占比 / R² / 斜率）
  node   pid 50392 · 增长 663.0 MB（761.9→1424.9）· 阶梯排除（最大单点跃升占净增长 83%）
```

**这正是方案预期结果**：

- pid 50392 的 30 min 增长 663 MB / +87% 过了所有数值门槛，但它的增长 100% 来自单次跃升（库里形态：36.7 MB 平台 25 min → 单点 686 MB → 平台 3 h → 单点 1425 MB）。只看净增长就会误报，阶梯规则把它挡下来了。
- pid 674 是典型的「Electron 主进程慢涨」（6 h +332 MB，窗口内 +8.4%），占比条件正确放过。

---

## 4. 端到端注入验证（真实库副本 + 合成泄漏序列）

往真实库副本里注入一条每点 +30 MB 的假进程（覆盖窗口内全部 10 个真实时间点），验证 `db::series` → `leak::detect` 整条链路：

```
注入 10 行（1792746860 → 1792748383）

== 判定结果 ==
参与 96 · 命中 1 · 良性 23 · 数据不足 14 · 未达标 58

== 命中明细 ==
leaky-worker [FakeApp] pid 99999 · 当前 651.5 MB · 30 min 增长 270.0 MB（+71%）
  · 斜率 605.5 MB/h · R² 0.960 · 阶梯占比 11%
```

同一个库里，注入的泄漏被抓出、真实的阶跃候选仍被拒绝 —— 说明规则在看数据，不是在看「有没有配置」。

---

## 5. 验收标准逐项对照

| # | 标准 | 结果 |
|---|---|---|
| 1 | 合成序列：爬升命中 / 阶跃拒绝 / 锯齿拒绝 / 平线拒绝 | ✅ 4 项单测 |
| 2 | pid 复用不接续成一条序列 | ✅ `series_splits_on_pid_reuse` + `reused_pid_series_are_judged_independently` |
| 3 | 良性名单（Chrome / Docker / 系统进程 / 自身）零命中 | ✅ 单测覆盖 6 类 |
| 4 | GC 语言门槛翻倍生效 | ✅ 三组对照单测 |
| 5 | 数据不足只计数不产出 item | ✅ 两项单测 |
| 6 | 真实库应为 0 命中，pid 50392 被阶梯规则拒绝且原因可见 | ✅ 探针输出如上 |
| 7 | 前端：无命中不出现提示条；命中显示增长量与「疑似」措辞 | ✅ 构建通过 + 代码复核（见 §6） |
| 8 | 全量门禁全绿 | ✅ 见 §1 |

---

## 6. 前端接线

| 位置 | 行为 |
|---|---|
| 面板「应用」tab | 仅命中时出现 `LeakBanner`：`疑似内存泄漏 · N 个应用 · M 个进程 · 30 min 合计 +X`，点击进主窗口；无命中不占版面 |
| 面板「概况」tab | 无命中且数据不足时脚注「需连续观测 30 min，当前 N 个进程数据不足」；库不可用时脚注说明原因 |
| 主窗口「应用内存」tab | 命中/无法判定时顶部出现 `LeakCard`：逐进程列证据（窗口、增长、占比、斜率、R²）+ 判据说明 |
| 家族行 | 命中家族加「疑似泄漏 +X」标记，紧凑版为「疑似泄漏」小字 |
| 检视器 | 选中命中进程时展示 `LeakEvidence`：窗口/净增长/斜率/R²/最大单点跃升/GC 门槛 |

文案遵守方案 §7.3：只说「疑似内存泄漏」，每条结论都带依据，不出现「释放 / 加速 / 优化」。

---

## 7. 已知限制（如实记录）

| 限制 | 影响 | 处置 |
|---|---|---|
| 首次运行前 30 min 无判定结果 | 新装用户半小时内看不到该类信息 | UI 显式显示「数据不足 N/30 min」，不假装没问题 |
| Go 运行时识别不完整 | 只认 `golang` / `go-build` 字样，编译后的 Go 二进制认不出 | 认不出时按普通进程判（门槛更低，只会多报不会漏报）；Node / Java 识别完整 |
| 探针无法拿到 exe | `leak-probe` 的良性判定只能按家族名/进程名匹配，比运行态更严 | 运行态由 `sampler` 传入实时进程表，exe 可用 |
| 家族级「总量泄漏」不会命中 | 若是多个进程各自缓慢小幅增长、家族总量可观，单进程门槛可能都不触发 | 已知取舍（D-L1）：家族级聚合在真实库上会误报 12.6 GB，代价更高 |
| 浏览器 UI 未做像素级确认 | `playwright-cli` 在本会话启动浏览器抛断言错误（环境限制，非代码问题） | 与 M1 相同处置：`pnpm build` + 接线复核；建议 `pnpm tauri dev` 实看一次 |

---

## 8. 与方案的实施偏差（已回扫进 `m3-leak-plan.md` §13）

| # | 方案原定 | 实际落地 | 原因 |
|---|---|---|---|
| 1 | GC 语言识别含裸词 `go` | 只认 `golang` / `go-build` | 裸 `go` 会命中 `Django`、`Google` 之类无关进程名 |
| 2 | 字段名 `slope_per_hour` | `slope_mb_per_hour` | 单位写进字段名，避免前端再猜一次 |
| 3 | 良性判定在数值判定之后 | **良性判定放在最前** | 否则 Chrome 这类会被计入 `insufficient`，脚注会误报「数据不足」 |
| 4 | 未定义 `start_time` 未知时的行为 | 退回用序列跨度判存续时长 | 库里可能有历史 start_time=0 的脏行 |
