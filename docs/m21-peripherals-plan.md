# M21 外设页（Peripherals）方案 v0.1

> 状态：**待确认**（决策点 D1–D5 未拍板，未实施）
> 前置调研：2026-11-20，基于 f26 硬件模块现状 + ps_sampler 链路核查

---

## 一、背景

### 1.1 需求

新增一个一级导航菜单，集中展示**接入本机的外部设备**状态，不限于耳机：

- 蓝牙外设：AirPods 等耳机（左耳/右耳/充电盒分件电量）、键盘、鼠标、触控板、手柄等
- USB 外设：U 盘、扩展坞、外置硬盘等有线设备
- 音频设备：当前输出/输入设备（哪个设备在出声）

### 1.2 现状盘点（已核查）

| 事实 | 位置 | 说明 |
|---|---|---|
| 一级导航 7 tab（⌘1–⌘7） | `src/windows/MainWindow.tsx:60–63` | `stats/resources/memory/disk/net/ports/device` |
| 「设备」页已含配件总电量 | `src/components/device/DeviceTiles.tsx`（PowerTile） | IOPS 数据源，仅"设备名 + 总电量%" |
| 蓝牙配件采样 | `src-tauri/src/ps_sampler.rs:222–229` | IOKit `IOPSCopyPowerSourcesInfo`，`Transport == "Bluetooth"` → `kind: "accessory"`；30s 低频 + active 门控 |
| 命令注册收口 | `src-tauri/src/lib.rs:80–170` `generate_handler![]` | 约 80+ 命令 |
| IPC 封装 | `src/lib/ipc.ts:605–656` | `invoke<T>` 薄封装 + `*_set_active` 门控命令 |
| 硬件开关白名单 | `src-tauri/src/hw.rs` `HW_KEYS` | `hw.power.enabled` 等 5 键 |
| 类型契约 | `src/types/hardware.ts:4–13` | `PowerSource { kind: "battery"\|"accessory" ... }` |

**结论**：IOPS 链路只能给"一个配件一个百分比"，拿不到 AirPods 左/右/盒分件电量、设备类型、连接状态、USB/音频信息。外设页需要新数据源，但采样纪律、命令注册、IPC 门控模式全部可复用。

### 1.3 与现有「设备」页的边界

| 页面 | 定位 | 内容 |
|---|---|---|
| 设备（device，现有） | **本机硬件 + 局域网** | 内置电池、磁盘 I/O/SMART、CPU 温度、局域网设备 |
| 外设（peripherals，新增） | **插上/连上本机的外部设备** | 蓝牙外设、USB 设备、音频设备 |

---

## 二、数据源调研

### 2.1 蓝牙外设

**主力：`system_profiler SPBluetoothDataType -json`（macOS 自带 CLI，零新依赖）**

| 字段 | JSON 键 | 说明 |
|---|---|---|
| 设备名 | `device_title` | 如 "许一的 AirPods Pro" |
| 已连接 | `device_isConnected` | `"Yes"/"No"`（顶层分 `connected` / 未连接两组，不同版本结构有差异） |
| 类型 | `device_type` | `"Audio"` / `Keyboard` 等；厂商 ID `device_vendorID`（Apple = 0x004C） |
| 主电量 | `device_batteryLevelMain` | 第三方耳机/手柄总电量 |
| 分件电量 | `device_batteryLevelLeft` / `device_batteryLevelRight` / `device_batteryLevelCase` | **AirPods 独有**，IOPS 拿不到的核心增量 |
| 地址/型号 | `device_address` / `device_productID` | 用于配对图标映射 |

风险：不同 macOS 版本 JSON 嵌套结构有差异（顶层分组/字段命名有变动历史）→ 用 `serde_json::Value` 防御式字段探测 + 全量兜底，不做强类型反序列化假设。

**补充：IORegistry 枚举**（`IOServiceMatching("AppleDeviceManagementHIDEventService")` 读 `BatteryPercent`）——覆盖妙控键盘/鼠标/触控板，与 ps_sampler 同款手工 IOKit extern，作为 system_profiler 失败/字段缺失时的兜底。

**不引入** IOBluetooth framework（`batteryPercentSingle` 属非公开属性，且需新增 objc2 绑定面）；BLE Battery Service（0x180F）对 AirPods 无效，放弃。

### 2.2 USB 设备

**`system_profiler SPUSBDataType -json`**：设备树（产品名、厂商、速率、所需功率、序列号）。与蓝牙共用同一子进程执行器，只是 DataType 参数不同。

### 2.3 音频设备

**CoreAudio 公开 API**（手工 extern，与 ps_sampler 手工声明风格一致）：

- `AudioObjectGetPropertyData` + `kAudioHardwarePropertyDevices`：设备列表
- `kAudioDevicePropertyStreams`（方向判输入/输出）、`kAudioDevicePropertyDeviceNameCFString`
- 默认设备：`kAudioHardwarePropertyDefaultOutputDevice` / `DefaultInputDevice`

输出：`{ name, direction: "output"|"input", isDefault }`。首次实现只做"列表 + 默认标记"，音量/采样率等细节放后续。

---

## 三、链路设计

```
┌─ Rust ─────────────────────────────────────────────┐
│ ep_sampler.rs（新模块，ep = external peripherals）  │
│   采样线程：15s tick + active 门控（沿用 ps 纪律）  │
│   ├─ spawn_blocking: system_profiler -json         │
│   │    ├─ SPBluetoothDataType → Vec<BTDevice>      │
│   │    └─ SPUSBDataType        → Vec<USBDevice>    │
│   ├─ CoreAudio extern          → Vec<AudioDevice>  │
│   └─ 快照 Mutex<Option<EpSnapshot>>（失败保持上轮）│
│   命令：ep_snapshot / ep_set_active                │
└────────────────────────────────────────────────────┘
                    ↓ invoke
┌─ 前端 ─────────────────────────────────────────────┐
│ types/peripheral.ts（类型契约）                     │
│ lib/ipc.ts（ep_snapshot / ep_set_active 封装）      │
│ stores/epStore.ts（zustand：快照 + set_active 生命周期）│
│ components/peripherals/PeripheralsBoard.tsx        │
│   ├─ BtSection（蓝牙卡片：分件 MiniGauge 环）       │
│   ├─ UsbSection（USB 卡片：设备列表）               │
│   └─ AudioSection（音频卡片：输出/输入 + 默认标记） │
└────────────────────────────────────────────────────┘
```

**采样纪律**（对齐 ps_sampler）：

- 15s 低频轮询；`ep_set_active` 门控——外设 tab 不可见时不采样，常驻开销 0
- 激活瞬间清 `last_at`，进 tab 立即补采一轮，不用干等
- system_profiler 单次 1–2s，放 `tokio::task::spawn_blocking`，不阻塞 async runtime；带 5s 超时兜底
- 失败语义：任一数据源失败 → 该分区保持上轮快照；**读不到不画 0**（设计语言硬规则）
- 开关：`hw.periph.enabled`（进 `HW_KEYS` 白名单，settings 可关）

---

## 四、设计决策点（待拍板）

| # | 决策点 | 选项 | 推荐 |
|---|---|---|---|
| **D1** | 新 tab 命名与数量 | ① 新 tab「外设」（8 tab，⌘1–⌘8）② 并入现有设备页 | **①**，用户已明确"新增菜单"；设备页保持"本机+局域网"不膨胀 |
| **D2** | 数据范围 | ① 蓝牙+USB+音频 ② 只蓝牙 ③ 蓝牙+USB+音频+外接显示器 | **①**；显示器（SPDisplaysDataType）信息价值低（和"关于本机"重叠），留 M3 后评估 |
| **D3** | 蓝牙数据源 | ① system_profiler 为主 + IORegistry 兜底 ② 纯 IORegistry | **①**，分件电量只有 system_profiler 给 |
| **D4** | 现有 PowerTile 配件行的去留 | ① 保留（设备页仍显示总电量）② 移除配件区、只留内置电池，配件归外设页 | **② 保留①现状先不改**，避免两页打架等 M3 实测后再定 |
| **D5** | 蓝牙 UI 形态 | ① 卡片分区（eul 式，逐设备一行/分件环）② 长列表 | **①**，对齐设计语言：百分比才套 MiniGauge 环，AirPods 左/右/盒各一个环 |

---

## 五、代码落点

### 5.1 Rust 侧

| 文件 | 动作 | 内容 |
|---|---|---|
| `src-tauri/src/ep_sampler.rs` | **新建** | `BTDevice`/`USBDevice`/`AudioDevice`/`EpSnapshot` 结构 + `run_ep` 采样线程 + `ep_snapshot`/`ep_set_active` 命令 |
| `src-tauri/src/lib.rs` | 修改 | `mod ep_sampler;` + `generate_handler!` 追加两条命令 |
| `src-tauri/src/hw.rs` | 修改 | `HW_KEYS` 追加 `hw.periph.enabled` |
| `src-tauri/Cargo.toml` | **不动** | 零新依赖（system_profiler 是子进程，CoreAudio 手工 extern） |

### 5.2 前端侧

| 文件 | 动作 | 内容 |
|---|---|---|
| `src/types/peripheral.ts` | **新建** | 类型契约（与 Rust serde camelCase 对齐） |
| `src/lib/ipc.ts` | 修改 | `epSnapshot()` / `epSetActive()` 封装（isTauri 兜底） |
| `src/stores/epStore.ts` | **新建** | 快照状态 + set_active 生命周期（对齐 lanStore 模式） |
| `src/components/peripherals/PeripheralsBoard.tsx` 等 | **新建** | 页面骨架 + 三个 Section（Bt/Usb/Audio） |
| `src/windows/MainWindow.tsx` | 修改 | `MainTab` 加 `"peripherals"`；TAB_ORDER/TAB_LABEL/TAB_HINT/⌘8/mapExternalTab（外部事件 payload `"peripherals"`） |
| `src/components/RailNav.tsx` | 修改 | 追加导航项与 hover hint |
| `src/i18n/`（zh/en） | 修改 | `nav.peripherals`、`peripherals.*` 配对 key 同步落地 |
| `src/panels/Island.tsx` | **不动** | 菜单栏面板本期不接（M3 后评估是否加外设 chip） |

### 5.3 类型契约草案

```rust
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BTDevice {
    pub name: String,
    pub device_type: String,          // "Audio" / "Keyboard" / ...
    pub connected: bool,
    pub is_apple: bool,               // vendorID == 0x004C
    /// None = 该部件读不到电量（前端不画环）
    pub battery_main: Option<u8>,
    pub battery_left: Option<u8>,     // AirPods 独有
    pub battery_right: Option<u8>,
    pub battery_case: Option<u8>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct USBDevice { pub name: String, pub vendor: Option<String>, pub speed: Option<String>, pub power_ma: Option<u32> }

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice { pub name: String, pub direction: String /* "output"|"input" */, pub is_default: bool }

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EpSnapshot {
    pub bt: Vec<BTDevice>, pub usb: Vec<USBDevice>, pub audio: Vec<AudioDevice>,
    /// 三个源各自的成功时间戳；单源失败保持上轮，前端按 collected_at 判新鲜度
    pub bt_at: Option<u64>, pub usb_at: Option<u64>, pub audio_at: Option<u64>,
}
```

### 5.4 UI 骨架（对齐 ui-eul-design-language）

- **BtSection**：已连接设备在前。AirPods 类 → 一行设备名 + 左/右/盒三个 MiniGauge 环（读不到的部件不画）；键盘/鼠标 → 设备名 + 主电量 MeterBar；未连接配对设备收进折叠"已配对未连接"分组
- **UsbSection**：紧凑列表（名称 · 厂商 · 速率），无电量概念不画环
- **AudioSection**：输出/输入两行，默认设备加 Badge 标记
- 全部空态走"隐藏是失败语义"：某分区无数据整块不渲染，绝不画"0 台设备"
- 卡片 `h-full min-h-[104px]`，文字三层以内，口径进 tooltip

---

## 六、边界风险

| # | 风险 | 缓解 |
|---|---|---|
| R1 | system_profiler 各 macOS 版本 JSON 结构漂移 | `serde_json::Value` 防御式探测 + 字段缺失即 None；失败保持上轮快照 |
| R2 | system_profiler 冷启动慢（1–2s，偶发更久） | spawn_blocking + 5s 超时；15s 低频下无感知；超时当本轮失败 |
| R3 | 非苹果蓝牙耳机多无电量上报 | `battery_main` 为 None → 该设备不画电量条，只显示名称与连接态 |
| R4 | IOPS 配件与 BT 列表重复显示同一设备（AirPods 在两页都出现） | D4 先保留现状；M3 实测后决定是否在设备页隐藏 accessory 行 |
| R5 | CoreAudio 手工 extern 声明出错（内存/生命周期） | 只用 GetPropertyData 只读查询 + CFString 规范释放；参考 ps_sampler 已验证的 CF 模式 |
| R6 | 8 tab 后 RailNav 拥挤 | rail 是竖排窄栏，7→8 变化小；⌘8 顺延；外部事件 payload 兼容白名单 |
| R7 | USB 设备树很深（hub 嵌套） | 只取叶子设备层，扁平化输出，忽略 hub 节点自身 |

---

## 七、里程碑

| 里程碑 | 内容 | 验证 |
|---|---|---|
| **M0 契约+蓝牙采样** | ep_sampler.rs（BT 部分）+ hw 开关 + 命令注册 + ipc.ts/types | `cargo check`/`clippy`；真机 `system_profiler` 输出比对 |
| **M1 外设页骨架+蓝牙区** | tab 接入 + RailNav + i18n + PeripheralsBoard + BtSection（含 AirPods 分件环） | 真机连 AirPods 验收分件电量；断连后列表收敛 |
| **M2 USB+音频区** | SPUSBDataType 采样 + CoreAudio 模块 + UsbSection/AudioSection | 插拔 U 盘 / 切换默认输出验证 |
| **M3 打磨+去重决策** | Island chip 评估、D4 去重实测、空态/失败态核查 | check-design.mjs 门禁全绿；验收清单逐项过 |

---

## 八、验收清单

- [ ] ⌘8 或点击 rail「外设」进入新页，⌘1–⌘7 原有 tab 不受影响
- [ ] 连接 AirPods：左耳/右耳/充电盒电量分别正确显示；任一分件读不到则该环不渲染
- [ ] 连接妙控键盘/鼠标：名称 + 电量正确显示
- [ ] 断开蓝牙设备后设备从"已连接"消失（不残留旧快照）
- [ ] 插入 U 盘后 USB 区出现对应条目；拔出后消失
- [ ] 切换默认音频输出设备后 Badge 跟随
- [ ] 蓝牙/USB/音频均无设备时对应分区整块隐藏，不显示"0 台"
- [ ] 外设 tab 切走后 `ep_set_active(false)`，采样停止（日志/时间戳验证）
- [ ] 设置里关闭 `hw.periph.enabled` 后页面分区全隐藏
- [ ] `cargo check` / `clippy` / 前端 typecheck / check-design.mjs 全绿
- [ ] i18n zh/en 配对 key 完整，无缺 key 报警

---

## 九、实施顺序约定

方案确认（D1–D5 拍板）→ 建 runbook → 按 M0→M3 实施 → 逐提交可编译分组（i18n 早于 UI 提交、精确路径 git add）→ 全绿门禁后合并。
