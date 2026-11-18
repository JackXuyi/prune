# F19 自耗透明（Prune 自身开销展示）方案

> 竞品依据（2026-11-18 联网核实）：Stats 社区长期吐槽其 CPU 占用 6–10%（iStat Menus 1–2%），
> 「监控工具自己吃了多少」已成为该品类的用户决策因素之一。Prune 的卖点是「口径不撒谎」，
> 对自己的开销同样不撒谎，是把卖点变成自证的机会——同时天然是一份可传播的对比页素材。

## 一、背景与目标

**背景**：Prune 是常驻菜单栏应用（30 s 端口轮 + 采样循环 + 灵动岛事件），
但用户没有任何地方能看到它自身的开销。竞品的教训（Stats）与口碑（iStat 低占用）都指向同一件事：
**自耗数字应当主动亮出来，而不是等用户拿活动监视器来抓**。

**目标**：设置页「关于」区展示 Prune 自身实时开销（内存口径 / CPU / 线程 / 常驻磁盘），
口径与 M2 的 `phys_footprint` 家族完全一致；README 增加自耗小节作为发布素材。

**成功标准**：

| # | 标准 |
|---|------|
| S1 | 设置页可见：物理内存占用（phys_footprint 口径）+ CPU%（采样轮均值）+ 线程数 + `prune.db` 磁盘占用，四项均带口径标注与数据时间 |
| S2 | 内存口径与内存 tab 对同名进程的显示一致（同一函数，零第二口径） |
| S3 | CPU 显示为**区间均值**而非瞬时值，避免「打开页面那一秒恰好高」的误读 |
| S4 | 不新增独立采样循环：读数挂在既有 sampler 轮上，增量开销 < 1 ms |
| S5 | README「自身开销」小节：静态口径说明 + 截图占位（数字随版本更新，不硬编码承诺上限） |
| S6 | `cargo test -p prune-core` 全绿 + `tsc` 0 错误 |

## 二、不做什么（本期边界）

- 不做自耗历史曲线、不做自耗告警（对自身做监控仪表盘是过度设计）
- 不承诺「永远低于 X MB」类上限（写死数字=未来打脸，只展示当下实测 + 口径）
- 不把 CLI / MCP 运行期开销纳入常驻口径（一次性进程，寿命 < 5 s，另列说明文字）
- 不做「自我清理」（如自动收缩 prune.db）——db 保留策略已有 M30 归档体系，不重复造

## 三、方案

### 3.1 读数核心（prune-core，纯函数可单测）

落在 `crates/prune-core/src/mem/self_footprint.rs`（新建）：

```rust
pub struct SelfFootprint {
    pub mem_bytes: u64,        // phys_footprint 口径（复用 mem/footprint.rs 同一函数）
    pub cpu_pct_avg: f32,      // 由调用方传入最近 N 轮均值，本模块不算
    pub threads: u32,
    pub db_bytes: u64,         // prune.db + wal 尺寸（paths.rs 已有定位）
    pub sample_note: String,   // 口径标注 i18n key + 采样时间
}

pub fn collect(db_path: &Path, cpu_avg: f32) -> Result<SelfFootprint, String>;
```

- 内存与线程读自身 pid（`std::process::id()`），复用 `mem/process.rs` 对单进程的既有采集路径
- **S2 的实现方式**：内存 tab 显示 Prune 自身时用的就是这份数据源，两侧同一函数，天然一致
- CPU 均值由 src-tauri sampler 轮在采集全机进程时顺带记下自身 pid 的值（环形缓冲 10 轮）

### 3.2 Tauri 命令（src-tauri）

- `commands.rs` 增加 `self_footprint() -> SelfFootprint`：sampler 轮已有数据 + db 尺寸即取即回，
  不触发新的进程扫描

### 3.3 前端（src/components/SettingsBoard.tsx 或 AboutSection.tsx）

- 「关于」区新增「Prune 自身开销」四行：内存 / CPU（10 轮均值）/ 线程 / 数据库磁盘
- 每行副文案标注口径：「phys_footprint 口径 · 与内存页同源」「打开设置时实测」
- 打开设置页时拉一次，不轮询（S4 纪律：这个数字不需要实时）

### 3.4 文档（README.zh-CN.md / README.md / 对比页素材）

- 新增「自身开销」小节：说明采集口径、常驻结构（菜单栏 + 30 s 只读轮）、
  一张设置页截图；**不写数值上限承诺**，写「在你机器上的实测值见设置页」
- 对比表加一行「自耗透明：展示自身开销 / 未展示」（沿用 M1 诚信归真的「我们做／未做」句式，无排他表述）

## 四、里程碑拆解

| # | 内容 | 验收 |
|---|------|------|
| T1 | prune-core：`mem/self_footprint.rs` + 单测（db 尺寸缺失容错 / 口径字段必带） | `cargo test -p prune-core` |
| T2 | src-tauri：sampler 记自身 CPU 均值（10 轮环形缓冲）+ `self_footprint` 命令 | `cargo check` |
| T3 | 前端：关于区四行 + 口径副文案 | `tsc` 0 错误 |
| T4 | 一致性核对：设置页内存数字 = 内存 tab 中 Prune 进程行 | 手工验证记录 |
| T5 | README 双语小节 + 对比表行 + 截图 | 文档评审 |

## 五、风险与对策

| 风险 | 对策 |
|------|------|
| 展示瞬间数字偏高（刚跑完首扫） | S3 十轮均值 + 副文案标「打开设置时实测」，不做过字段解释 |
| 数字被竞品 / 用户截屏断章取义 | 展示永远带口径与时间戳；不承诺上限；README 明确「随版本与机器而异」 |
| Tauri WebView 子进程开销不计入主 pid | 口径诚实：聚合主进程 + WebView 子进程（同 family 遍历），子进程缺失时单独标注「仅主进程」 |
| 后续采样结构变化导致均值来源丢失 | CPU 均值缺失时显示「暂无数据」而非 0%（S3 同一纪律：不撒谎） |

## 六、验收清单

- [ ] 四项读数齐全且各带口径标注与时间（S1）
- [ ] 设置页与内存 tab 对 Prune 自身的内存数字一致（S2，实测核对）
- [ ] CPU 为区间均值，缺失时显示「暂无数据」不显示 0%（S3）
- [ ] 无新增采样循环，读数增量 < 1 ms（S4，代码走查确认）
- [ ] README 双语小节 + 对比表「我们做／未做」行，无数值上限承诺（S5）
- [ ] 全文无「最快 / 最省 / 永远低于」类绝对化表述
- [ ] `cargo test -p prune-core` 全绿 + `tsc` 0 错误（S6）
