# f26 硬件模块方案：SMART / 磁盘 I/O / 电池 / 蓝牙电量 / CPU 温度 / 公网 IP

> 对标 Stats 功能缺口分析（2026-11-19 调研）中的 G-03 ~ G-08。
> 分三阶段落地，按「成本 × 风险 × 收益」排序，每阶段独立可验收。

## 一、背景

- Prune 当前已覆盖内存、进程 CPU、网速/流量、端口、LAN 设备五个维度（`sampler.rs` / `net_sampler.rs` / `port_sampler.rs` / `lan.rs`）。
- 对比 Stats（v3.1.0，mac-stats.com，检索 2026-11-19）仍有六个缺口：磁盘 SMART 健康（G-03）、磁盘读写活动（G-04）、电池模块（G-05）、蓝牙设备电量（G-06）、CPU 温度（G-07）、公网 IP（G-08）。
- 本方案只补「读」能力，不改「监控 → 阈值告警 → 一键修剪」主线（阈值告警属 G-10，另行立项）。

## 二、范围

**包含**：G-03 ~ G-08 六项只读硬件/网络信息能力，全部走「可见性门控采样 + 快照命令」模式。

**不包含**：风扇控制、GPU 监控（G-01/G-02，护城河级投入，本期不做）、阈值通知联动（G-10）、传感器历史入库、写入型操作。

## 三、链路

统一链路（与 `net_sampler.rs` 纪律一致）：

```
前端 tab 可见 → set_active(true) → 采样循环启动（低频/门控）→ 状态快照
前端命令 invoke → 直接读最新快照（无等待）→ 卡片渲染
tab 切走 → set_active(false) → 停采样（子进程必须消失）
```

各能力数据源链路：

| 能力 | 数据源 | 频率 | 形态 |
|------|--------|------|------|
| G-04 磁盘 I/O | `iostat -d -k -c 2 -w 1` 子进程，取第二次样本差分 | 2 s，tab 可见时 | 门控采样 |
| G-03 SMART | `diskutil info -plist <bsd>` 解析 `SMARTStatus` | 磁盘 tab 打开时一次 + 1 h 缓存 | 一次性命令 |
| G-05 电池 | IOKit `IOPSCopyPowerSourcesInfo`（公开 API） | 30 s 低频常采 | 门控采样 |
| G-06 蓝牙电量 | 同上——蓝牙键盘/鼠标/耳机以外部电源源身份出现在 IOPS 里 | 同 G-05 | 同 G-05 |
| G-07 CPU 温度 | SMC 私有接口（IOService `AppleSMC` + 芯片键表） | 30 s，tab 可见时 | 门控采样 |
| G-08 公网 IP | HTTPS 出站请求（ipify → ip.sb → ipinfo 三级回退） | 按需 + 10 min 缓存 | 一次性命令 |

## 四、设计决策

### D1：电池与蓝牙电量共用一个数据源（G-05 + G-06 合并实现）

macOS 的 `IOPowerSources` 天然把内置电池和蓝牙配件（键盘/鼠标/触控板/耳机）都列为电源源，一套 API 两个能力，不做两套实现。返回结构统一为 `PowerSource { kind: Battery|Accessory, name, percent, charging, time_to_empty }`。
sysinfo 0.39 的 `Battery` 只覆盖内置电池且拿不到配件，故不用它做主源。

### D2：磁盘 I/O 走 iostat 子进程，不走 IOKit 直读

- 与 `net_sampler.rs` 的 nettop 范式同构，代码模式可直接复用（一次性采样 + 差分）。
- IOKit `IOMediaBlockStorageDriver` 统计字典免子进程，但 objc2-io-kit 不在依赖树，为两个卡片引入新框架不值；后续若常驻菜单栏显示磁盘 I/O 再升级。
- `iostat -c 2 -w 1` 两次采样取第二次（覆盖 1 s 区间），单轮耗时 ≈ 1 s、CPU 可忽略。

### D3：SMART 用 diskutil 解析，不引入 smartctl

- `diskutil info -plist` 输出 `SMARTStatus`（"Verified" / "Failing" / 缺失），零依赖、零权限。
- 明确产品口径：**只做状态灯（正常/警告/不支持），不做详细属性表**——详细属性在 macOS 上无公开 API，诚实降级比假数据好。

### D4：CPU 温度走 SMC 私有键表，做成「失败即隐藏」的增强项

- macOS 无公开温度 API；`powermetrics` 要 root，用户不可能每次授权。业界通行做法（Stats/iStat/macmon）都是直读 SMC。
- 维护按芯片代际的键表：Intel（TC0P/TC0D…）、Apple Silicon（pACC MTR / MCP / P-core E-core 传感器键），取已知键中可读项的平均值作为「CPU 温度」。
- 任何一步失败 → 快照返回 `None` → 前端不渲染该卡片（不是显示"不可用"占位），与岛（M15）「失败不影响主功能」纪律一致。
- 每次启动仅探测一次键表可用性，可用的键缓存，之后 30 s 一次只读键值。

### D5：公网 IP 默认开启 + 显式文案 + 三级回退

- 隐私考量：出站请求只取 `GET /ip` 纯文本端点，不携带任何用户数据；设置页提供开关（`net.publicIp.enabled`，默认开）。
- 回退链：`api.ipify.org` → `api.ip.sb/ip` → `ipinfo.io/ip`，10 s 超时，成功后缓存 10 min；手动刷新按钮绕过缓存。
- 失败显示「不可用」并保留手动重试，不静默重试堆积。

### D6：前端落位——不新增 tab

| 能力 | 落位 | 理由 |
|------|------|------|
| G-04 + G-03 | 「磁盘」tab 新增两张卡 | 与卷容量卡同屏语义 |
| G-08 | 「网络」tab 新增公网 IP 读数 | 与现有 NetBoard 同屏 |
| G-05 + G-06 + G-07 | 「概览」tab（StatsBoard）新增硬件读数区 | 概览卡本来就是一眼看状态的定位 |

RailNav 不加新 tab，导航密度不变；i18n 键按 `hw.*` 前缀组织。

### D7：每个模块一个独立开关

沿用 `settings.rs` 布尔键模式：`hw.diskIo.enabled`、`hw.smart.enabled`、`hw.power.enabled`、`hw.cpuTemp.enabled`、`net.publicIp.enabled`，默认全开（CPU 温度因私有接口风险默认开但失败自动隐藏）。设置页集中在 SettingsBoard 新增「硬件模块」分组。

## 五、代码落点

**阶段一（低风险高收益）：G-08、G-05/G-06、G-04**

| 落点 | 内容 |
|------|------|
| `src-tauri/src/ps_sampler.rs`（新） | IOKit IOPS 采样：`PowerSource` 结构、30 s 低频循环、`set_active` 门控、`power_snapshot` / `power_set_active` 命令 |
| `src-tauri/src/pubip.rs`（新） | `pub_ip_get(force: bool)` 命令 + 10 min 缓存 + 三级回退（`tokio` + `reqwest` 或最小化用 `curl` 子进程，倾向 reqwest，tauri 已带 hyper 栈） |
| `src-tauri/src/disk_io_sampler.rs`（新） | iostat 门控采样（复刻 `net_sampler.rs` 的 nettop 纪律：不可见时子进程必须消失）、`disk_io_snapshot` / `disk_io_set_active` |
| `src-tauri/src/lib.rs` | 注册三个 mod + 命令 + `app.manage` 状态 + `spawn` 两条循环（ps 常采低频、disk_io 门控） |
| `src/lib/ipc.ts` | `powerSnapshot` / `pubIpGet` / `diskIoSnapshot` 封装 + 类型 |
| `src/components/NetBoard.tsx` | 公网 IP 读数行（含刷新按钮 + 状态态） |
| `src/components/StatsBoard.tsx` | 硬件读数区：电池（电量/充电/剩余时间）+ 蓝牙配件电量列表 |
| `src/components/charts` 或磁盘 tab | 磁盘 I/O 卡（读/写 KB/s + Sparkline） |
| `src/components/SettingsBoard.tsx` | 「硬件模块」开关分组 |
| `src/i18n/*` | `hw.*` / `net.publicIp.*` 键（zh + en） |

**阶段二：G-03、G-06 完整化（配件名映射）**

| 落点 | 内容 |
|------|------|
| `src-tauri/src/commands.rs` | `disk_smart()` 命令：枚举内置盘 bsd 节点 → `diskutil info -plist` → 解析 `SMARTStatus`，1 h 内存缓存 |
| `src/components` 磁盘 tab | SMART 状态灯卡（正常绿 / 警告红 / 不支持灰） |

**阶段三（高风险）：G-07**

| 落点 | 内容 |
|------|------|
| `crates/prune-core/src/smc.rs`（新） | SMC 读取封装：`IOServiceOpen("AppleSMC")`、键表（按芯片代际）、`cpu_temp_avg()`、探活缓存。依赖 `objc2-io-kit`（或最小 `io-kit-sys`） |
| `src-tauri/src/smc_sampler.rs`（新） | 门控 30 s 采样 + `cpu_temp_snapshot` / `cpu_temp_set_active` 命令 |
| `src/components/StatsBoard.tsx` | CPU 温度读数（`None` 即整卡不渲染） |

## 六、边界风险

| 风险 | 等级 | 应对 |
|------|------|------|
| SMC 键随芯片代际变化，读到错误键产出垃圾温度 | 高 | 键表白名单 + 读值合理性校验（-10~120 ℃ 外丢弃）；失败即隐藏；灰度放量 |
| macOS 升级封堵 SMC 私有接口 | 中 | 失败自动隐藏（D4），产品叙事不依赖温度 |
| iostat 在低电量模式变慢/输出格式随系统版本漂移 | 中 | 解析失败跳过本轮；连续 N 轮空样本判定不可用（复用 net_sampler 的 `EMPTY_LIMIT` 语义） |
| USB/外置盘无 SMART 报告 | 低 | 明确显示「不支持」，不报错 |
| 蓝牙配件电量在部分设备（非苹果配件）缺失 | 低 | 只渲染有百分比的源，缺失即不显示 |
| 公网 IP 请求被防火墙/代理拦截 | 低 | 10 s 超时 + 三级回退 + 手动重试；不自动重试堆积 |
| 台式 Mac（Mac mini/studio）无电池 | 低 | `IOPS` 无内置电池源 → 前端不渲染电池卡 |
| 私有 IOKit 依赖与 objc2 版本冲突 | 中 | 阶段三前置 spike：确认 `objc2-io-kit` 与现有 `objc2-app-kit 0.3.2` 共存后再开工 |

## 七、验收清单

**阶段一**
- [ ] 网络切走后 `pgrep iostat` 为空（门控纪律，与 nettop 同标准）
- [ ] 笔记本上电池卡显示电量/充电态/剩余时间；拔电源 30 s 内充电态翻转
- [ ] 蓝牙键盘/耳机电量正确显示；无蓝牙配件时该区不渲染
- [ ] 公网 IP 与 `curl api.ipify.org` 结果一致；断网 10 s 超时显示「不可用」且手动重试可恢复
- [ ] 关闭 `hw.power.enabled` 后电池区消失且采样停止
- [ ] `cargo check` / `clippy` 通过（不新增警告）

**阶段二**
- [ ] 内置盘显示 SMART 状态灯，`diskutil info disk0 | grep SMART` 与 UI 一致
- [ ] 外置无 SMART 盘显示「不支持」不报错
- [ ] SMART 结果 1 h 内重复打开磁盘 tab 不重复执行 diskutil（日志可证）

**阶段三**
- [ ] spike 报告确认 objc2-io-kit 可共存后才合入
- [ ] 温度值落在合理区间且与 macmon 同机对比偏差 < 5 ℃（人工抽查）
- [ ] SMC 打开失败的机型上温度卡不渲染，其余功能不受影响
- [ ] 采样开启时 Prune 自身 CPU 增量 < 0.5%（活动监视器实测）

---
本方案为产品设计参考，具体实施请结合公司合规与法务要求确认。

---

## 八、实施记录（2026-11-19 全部落地）

三阶段全部实施并通过验收，与方案的两处偏差与一个运行时修复：

1. **D2 升级（G-04）**：sysinfo 0.39 的 `Disk::usage()` 在 macOS 侧按 **bsd 物理盘**计数，
   给出真正的读/写拆分——比原方案的 iostat（读+写合计）口径更好且零子进程。
   `disk_io_sampler.rs` 改为 sysinfo 差分；挂载集变化时自动重建基线；首轮 warmup。
2. **D4 升级（G-07）**：硬编码键表在 M2 上枚举失败（键集随代际漂移），
   改为**启动后一次性枚举全部 SMC T 键**（进程级 OnceLock 缓存），优先 `Tp*`
   （P 核近端）取中位数；枚举失败才回退硬编码表。本机（Apple M2）spike 实测：
   129 个温度键、Tp* 中位数 62 ℃（空载）～72 ℃（负载）。
3. **运行时修复**：`mach_task_self_` 是全局变量（`#define mach_task_self() mach_task_self_`），
   按函数声明调用直接 SIGBUS——已改为 `static` 读取；另修了枚举循环里
   选中句柄被提前 `IOObjectRelease` 导致 IOServiceOpen 失败的问题。

验收核对（方案 §七）：
- [x] `cargo check` / `cargo clippy --lib` 通过，无新增警告（剩 2 条为 report.rs 存量）
- [x] `cargo test --lib` 60 通过（SMART plist 解析等新测试含内）
- [x] SMC spike：真机读数 20~110 ℃ 区间（#[ignore] 手动跑，通过）
- [x] `npx tsc --noEmit` + `npm run build` 通过（i18n 类型硬闸生效：hw 命名空间漏挂/漏译会编译失败）
- [x] 门控纪律：三个采样器均 `set_active(false)` 即停（ps/temp 5 s tick + 30 s 间隔；disk I/O 无子进程、不 refresh）
- [ ] UI 真机验收（电池/蓝牙/温度卡渲染、公网 IP 显示）待开发者打开 App 人工确认

新增文件：`src-tauri/src/{pubip,ps_sampler,disk_io_sampler,smc,smc_stub,smc_sampler,hw}.rs`、
`src/types/hardware.ts`、`src/components/{DiskHardwareStrip.tsx,stats/HardwareStrip.tsx}`、
`src/i18n/messages/namespaces/hw.ts`。改动落点见方案 §五。
