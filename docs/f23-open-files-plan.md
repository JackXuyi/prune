# F23 Open Files 占用反查（谁占着这块盘）

> 状态：方案（未实施）。P1，只读反查，符合「不做跨应用清理」边界。
> 竞品依据：Vitals 1.11（2026-11）验证高频痛点——「外置盘弹出不了，谁占着文件」；
> macOS 系统只给一个模糊的「有文件正在使用」。

## 一、背景

- 外置盘 / U 盘弹出失败时，macOS 只说「因为它包含正在使用的文件」，不说是谁。
  `lsof +D <挂载点>` 能答，但普通用户不碰命令行。
- Prune 的定位（开发机资源守门员 + 只读观测）与该诉求完全同构：把「谁占着」
  从命令行搬到页面，**只查不解**——解除占用（关闭 App / force eject）是用户的决定。
- 与 F16 why 同一家族：结构化证据 → 一句人话。

## 二、成功标准

1. GUI 选一个挂载点（或拖入路径）→ 秒级返回占用者列表（进程 / pid / 占用文件数 / 代表文件）。
2. CLI `prune files <path>` 同一结论；MCP 本期不做（§4.3 显式取舍）。
3. 扫描只读：零写入、零 kill、零 eject 动作。
4. 大挂载点（Time Machine 备份盘）有明确耗时预期与取消，不假死。

## 三、不做什么

- **不做「解除占用 / 强制弹出」按钮**——只读纪律；「安全弹出」属于改状态。
- **不做文件级实时监视**（FSEvents / kqueue）——常驻监视违背最小打扰；按需单发查询。
- **不做网络盘 / NFS 深挖**——lsof 对 NFS 的输出不可靠，第一版只声明本地盘支持。
- **不做 MCP 工具**（本期）——agent 场景可跑 `prune files`；工具面扩张成本暂不付。

## 四、方案

### 4.1 核心层（prune-core/src/disk/open_files.rs）

- 命令：`lsof -F pcn0 -t -- <mount>` 机器格式（`-F pcn0` 与 ports.rs 同家族解析；
  `-t` 只列挂载点相关的进程？——**实测后定**：备选 `lsof -F ptcn0` 全量 + 前缀过滤，
  两条路径都写进解析单测，落地时按实测耗时选，快的那条进方案，慢的留注释）。
- 解析复用 ports.rs 的 `-F` 块式解析经验（P/c/n 行 + fd 块），新增类型行过滤：
  `-F t` 给文件类型，**只留 VREG**（普通文件）——目录/socket/pipe 不算「占用着文件」。
- 输出纯结构：

```rust
pub struct OpenFilesReport {
    pub path: String,             // 归一化挂载点/目录
    pub holders: Vec<Holder>,     // 按占用文件数降序
    pub scanned_ms: u128,
    pub coverage_note: Option<String>,  // 无权限归属的进程数（与端口 coverage 同纪律）
}
pub struct Holder {
    pub pid: i32,
    pub comm: String,
    pub files: u32,
    pub sample_files: Vec<String>,   // ≤3 条代表路径，给人看
}
```

- **Coverage 纪律**：无权限读不到的进程如实计数（「另有 N 个进程的文件句柄不可见，
  可用 sudo 复查」），不谎报「无人占用」——这与 F16 why 的「确认空闲」三态同源。

### 4.2 性能与取消

- 挂载点级查询在 spawn_blocking；GUI 命令带 5 min TTL 缓存（`AppDiskState` 同款模式，
  `commands.rs` 已有缓存先例）；重复点击走缓存零开销。
- 超 5 s（Time Machine 级大目录）→ 前端显示「扫描中…（大目录可能需要十余秒）」，
  本期不做中途取消（lsof 子进程 kill 会留半截结论），以预期管理替代。

### 4.3 消费面

- **GUI（磁盘 tab 卡片）**：「占用反查」卡：路径选择（挂载点下拉 = 已挂载的外置卷，
  来自 `disk` 模块既有卷枚举；手输路径兜底）→ 结果列表（Holder 行：comm + pid +
  files 数 + 样例路径 hover）→ 空态「没有可见的占用者（含权限盲区说明）」。
  占用者行**不给 kill 入口**——「定位进程」复用 App 面板跳转（只看不执行）。
- **CLI**：`prune files <path> [--json]`——**新子命令**（cli-reference 11→12，契约
  + README + check:docs 三处同变更）。退出码沿用家族惯例（主输出空 + coverage note → 3）。

## 五、里程碑

| 里程碑 | 内容 | 验证 |
|---|---|---|
| M-A | lsof 调研定案（`+D` vs 全量过滤实测耗时）+ 解析纯函数 + 单测（含 VREG 过滤、coverage 盲区） | cargo test |
| M-B | CLI `prune files` + 契约三处同变更 | cargo test + check:docs |
| M-C | `open_files_scan` 命令 + TTL 缓存 + 磁盘 tab 卡片（挂载点下拉 + 结果列表） | tsc + 真机 |
| M-D | 门禁全套 | check:* 全绿 |

## 六、风险与对策

- **lsof 全量慢**（>2 s）：M-A 实测决定路径；缓存 + 预期文案兜底，不假死不取消。
- **权限盲区误判「可以弹出」**：coverage_note 永远随结果输出；GUI 空态文案写
  「没有**可见的**占用者」，「可见的」三个字是口径底线。
- **路径含空格/中文**：`-F pcn0` 的 n 行以 NUL 结尾（`0` flag 的意义），解析按 NUL 切，
  空格天然安全；单测带中文路径样例。
- **挂载点枚举差异**（APFS 快照挂载 / 未挂载 dmg）：下拉只列「已挂载的真实卷」，
  dmg/快照过滤掉；手输路径不校验卷类型，lsof 自己会答。

## 七、验收清单

- [ ] 单测：VREG 过滤、NUL 分隔解析（含空格/中文路径）、coverage 盲区计数、holders 降序
- [ ] 真机：插入 U 盘 → `open` 一个文件 → 扫描命中该 App；关闭后重扫消失
- [ ] 真机：无权限进程占用（如 root 服务）→ 出现在盲区计数而非静默丢失
- [ ] CLI：`prune files /Volumes/X` 人话输出；`--json` 进冻结快照；空输出 + 盲区 → 退出码 3
- [ ] GUI：大目录 >5 s 显示预期文案；5 min 内二次点击零开销（日志可证）
- [ ] 门禁全套绿；全文无「解除/弹出/结束」执行类动词（check:docs sweep 可证）

## 八、实现记录（2026-11-18）

- 落点：`disk/open_files.rs`（scan + parse 纯函数，4 单测）/ CLI `prune files <PATH> [--json]`（新子命令 11→12，契约 files 键 + cli-reference 章节）/ `open_files_scan`（TTL 5 min 缓存，OpenFilesState）+ `open_files_volumes`（/Volumes 枚举）命令 / 前端 `OpenFilesCard`（磁盘 tab 顶部，卷下拉 + 手输兜底 + 占用者列表 + 盲区说明常驻）。
- M-A 真机定案：全量 `lsof -F ptcn0` + 前缀过滤（≈230–340 ms，与目录大小无关），弃 `+D`。
- **真机踩坑两枚（比方案多出的纪律，已写进代码注释与单测）**：
  1. macOS lsof 文件类型是 `tREG`，GNU 是 `tVREG`——两者都收（否判定为空）；（f23 §六预判过解析风险，实测比预判更早暴露）
  2. lsof 输出**解析后的真实路径**（/tmp → /private/tmp）——查询路径必须先 canonicalize 到同一坐标系，否则前缀匹配永远落空。
- 真机验证：`prune files /tmp` 空态 + 盲区说明；`sleep 20 > /tmp/x` 占用场景命中 sleep；workbuddy sandbox lock 文件命中 BackgroundTaskManagementAgent。
- 门禁：core 532 / cli 59 全绿；tsc / check:i18n / check:docs（12 子命令对账）通过；占用者行无任何执行入口（check:docs 执行类动词 sweep 覆盖）。
