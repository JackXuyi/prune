# F5 局域网新面孔提醒（M23）方案

> 前置：M18 局域网设备（docs/m18-lan-devices-spec.md）提供了完整的数据底盘。
> 本功能是「把已有读数变成可玩内容」系列的第四块（M19 端口 → M20 宠物 → M21 成绩单 → M22 卷宗 → M23 新面孔）。

## 1. 目标与定位

有新设备接入 Wi-Fi 时，灵动岛轻提报一句「咦，一个新设备 join 了」，
点击直达 ports tab 的局域网设备区（LanBoard 挂在 PortList 里）。

**定位边界（继承 M18 §1）**：本功能是「邻居表观察者」，不是网络扫描器——
- **零主动探测**：不跑 probe_sweep、不发 M-SEARCH、不浏览 mDNS（那些是用户主动触发的 35s 短任务）
- 数据源只有 `arp -an`（内核邻居表）+ 默认路由网卡信息，~百毫秒级、每分钟一次
- 不做 OS 指纹、不请求对方任何服务（M18 §7.2 越界红线原样生效）

## 2. 判定规则

### 2.1 设备身份键：IP（继承 M18 spec §9）

随机化 MAC 是常态（本机实测 10 条邻居里 5 条随机化），设备聚合键统一用 IP。
代价（明确接受，写入 changelog）：
- 设备换 DHCP 地址 → 会误报一次「新面孔」
- 新设备恰好拿走旧 IP → 漏报一次

### 2.2 状态机：基线 + 边沿检测（与 M19 PortGuardMonitor 同构）

```
首轮（或网段变化后首轮）  → 只建基线，一条不报
之后每轮                  → online 且 !is_self 且 !is_gateway 且 IP ∉ 已知集 → LanWatchAlert
                            该 IP 立即入已知集（同一设备终身只报一次）
```

三条防刷屏纪律：
1. **基线不补报**：Prune 启动时邻居表里的存量设备全是旧面孔
2. **网段变化即重置**：CIDR 变了（换了 Wi-Fi / 插网线）→ 重建基线。
   新网络里「全是新面孔」不是用户想听的故事，一条都不弹
3. **单轮上限 3 条 + 余量合并**：一轮冒出 >3 台新设备（极端场景）时，
   只报前 3 条、最后一条带 `extra: N`（「还有 N 台」）；绝静默截断

### 2.3 只看「在线」设备

`LanDevice.online`（内核可达态或组播响应）。Incomplete 无响应的幽灵条目不报。

## 3. 提醒通道

灵动岛第四类事件 `kind: "lan"`，复用 M19 模板：
- 折叠态：绿点图标（#30D158，第四种强调色）+ 标签「新面孔」+ IP 尾段
- 展开卡：设备名（hostname > vendor > IP）+ 类型词 + 随机化 MAC 提示
- 点击 → `openMain(undefined, "ports")`（LanBoard 在 ports tab）
- 纪律不变：不常驻、不抢焦点、6 秒自走、展示中后到事件覆盖前者

## 4. 数据流

```
lan_watch_loop（60s 一拍，岛关/开关关 → 整拍跳过，常驻开销 0）
  ├─ spawn_blocking:
  │    lan::local_scope()          // route + ifconfig，无网段 → 跳过本拍
  │    lan::neighbors()            // arp -an（macOS）/ GetIpNetTable2（Windows）
  │    lan::merge(&nbrs, &[], &[], &[], gateway, me, scope)   // 零探测、零 lsof
  ├─ LanWatchMonitor::observe(now, cidr, &devices) → Vec<LanWatchAlert>
  ├─ RateLimiter（独立 4/h 预算，不与端口/CPU/内存共闸）
  └─ fire_lan → deliver（island://alert）
```

刻意不接 lsof（inbound 端口对「谁新来了」没有信息量，少一次 ~300ms 的子进程）。

## 5. 设置

- `island.lan_watch`（默认开），设置面板灵动岛区第四个开关「局域网新面孔」
- `IslandSettings` 前后端同次变更（防 serde 断契约，M19 教训）

## 6. 里程碑

| 阶段 | 内容 | 验收 |
|------|------|------|
| T1 | 核心层 `LanWatchMonitor`（prune-core/src/island.rs）+ 单测 | cargo test 绿 |
| T2 | Tauri `lan_watch_loop` + `fire_lan` + cfg/settings 接线 | cargo check 绿 |
| T3 | 前端 Island.tsx lan 事件折叠/展开卡 + SettingsBoard 开关 | tsc 0 错误 |
| T4 | 全量回归 + fmt + README/CHANGELOG | 全绿 |
| T5 | 真机验证（手机连 Wi-Fi → 岛弹报）| 手工 |

## 7. 风险与对策

| 风险 | 对策 |
|------|------|
| 邻居表 expiry 导致设备「消失又出现」反复弹 | 已知集终身制（入集不_remove），同 IP 只报一次 |
| 公司网络 / 大网络设备潮 | 单轮 3 条上限 + 4/h 独立频控 + 网段变化重置 |
| DHCP 换址误报 | 接受并在展开卡措辞上保持「新面孔」的中性语气（不是「入侵」）|
| 电池影响 | 60s × arp/route/ifconfig（无 lsof），占空比 <0.5% |
