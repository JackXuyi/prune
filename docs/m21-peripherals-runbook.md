# M21 外设页 runbook

> 方案：docs/m21-peripherals-plan.md（D1–D5 按推荐项执行）
> 节奏：M0 → M1 → M2 → M3，逐提交可编译；i18n 早于 UI 提交

## M0 Rust：ep_sampler + 命令 + 开关

1. `src-tauri/src/ep_sampler.rs` 新建：
   - `BTDevice`/`USBDevice`/`AudioDevice`/`EpSnapshot`（serde camelCase）
   - 蓝牙 + USB 走 `/usr/sbin/system_profiler <DataType> -json`（spawn_blocking + 6s 超时），`serde_json::Value` 防御式解析
   - 蓝牙结构（真机 2026-11-20 实测）：`SPBluetoothDataType[].device_connected / device_not_connected`，条目 `{ "<名>": { 属性 } }`；电量键 `device_batteryLevelMain/Left/Right/Case`（字符串，可能带 %）
   - USB 结构：`SPUSBDataType[]` 总线树，`_items` 递归，叶子判据 `manufacturer || product_id`
   - 音频：CoreAudio 手工 extern（`AudioObjectGetPropertyData` + `kAudioHardwarePropertyDevices/Streams/DefaultOutput/InputDevice` + `kAudioObjectPropertyName`），非 macOS 走 stub
   - 纪律对齐 ps_sampler：15s 低频 + 5s tick 门控 + active 开关；单源失败保持上轮时间戳
2. `hw.rs`：`HW_KEYS` 加 `hw.periph.enabled`（注释 五个→六个）
3. `lib.rs`：`mod ep_sampler` + `manage(EpState)` + `spawn run_ep` + `generate_handler` 两条命令
4. 门禁：`cargo check` + `cargo clippy`

## M1 前端契约 + 外设页骨架 + 蓝牙区

1. `src/types/peripheral.ts` 新建类型契约
2. `src/lib/ipc.ts`：`epSnapshot` / `epSetActive`（f26 区段末尾追加）
3. `src/i18n/messages/namespaces/peripherals.ts` 新建（zh 先行）+ nav.ts 加 `peripherals`/`peripheralsHint`；zh-CN.ts / en-US.ts 注册
4. `src/components/icons.tsx`：加 `HeadphonesIcon` / `UsbIcon`（细线 eul 风格）
5. `src/components/peripherals/usePeripherals.ts`：hwFlags 门控 + set_active + 15s 轮询（对齐 useHardwareReadouts）
6. `src/components/peripherals/PeripheralsBoard.tsx` + `BtSection`：AirPods 左/右/盒 MiniGauge 环（读不到不画）；主电量走 MeterBar；空卡整块隐藏
7. 门禁：`npm run typecheck`

## M2 tab 接入 + USB/音频区 + 门禁全绿

1. `MainWindow.tsx`：`MainTab` 加 `"peripherals"`、TAB_ORDER/TAB_LABEL/TAB_HINT、渲染分支、⌘1–8、刷新按钮隐藏条件
2. `RailNav.tsx`：RailTab + ITEMS 追加（`UsbIcon`，域色 `--color-domain-device`）
3. `UsbSection` / `AudioSection`（默认设备 Badge）
4. 门禁：typecheck + `npm run build` + `node scripts/check-design.mjs`

## M3 提交分组 + 收尾

1. commit 1（Rust）：ep_sampler + lib.rs + hw.rs + runbook/plan 文档
2. commit 2（前端契约）：types/ipc/i18n
3. commit 3（UI）：icons/peripherals 组件/MainWindow/RailNav
4. 真机验收：AirPods 分件电量、U 盘插拔、默认输出切换
5. 记忆日志落档

## 验收清单

见 docs/m21-peripherals-plan.md §八，逐项过。
