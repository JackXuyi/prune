# f12 · M28 Xcode 深扫专项（残余差距）

> 2026-11-13 · 承接 [f11](./f11-ai-cache-2-plan.md) 的方法论：先本机实测、逐目录核实内容性质，再判定登记级别。
>
> **开工核验结论**：M28 四组中三组已落地——DeviceSupport（M11）、Archives（M6）、模拟器旧运行时（0.8.3 `simulator-runtimes`），且 **CoreSimulator/Devices（key `simulator`，L3）与 CoreSimulator/Caches 早已登记**，本机 3.5 GB 最大头已有覆盖。真正的残余是 **DerivedData 一条大项 + 四条随时间增长的小项**，共 5 条。按 roadmap「与 M27 同版本可并行」，合入 0.8.5 发布。

## 1. 本机实测（2026-11-13，macOS）

| 路径（home 相对） | 体积 | 内容性质 | 判定 |
|---|---|---|---|
| `Library/Developer/Xcode/DerivedData` | **68 MB** | 构建产物 + 索引 + 日志，重新编译全部可再生，不含用户创作内容 | 🟢 L2 |
| `Library/Developer/CoreSimulator/Temp` | 0 B（会增长） | 模拟器运行临时文件 | 🟢 L2 |
| `Library/Developer/DeveloperDiskImages` | 17 MB | 连真机时下载的设备磁盘映像，按 iOS 版本可重下 | 🟢 L2 |
| `Library/Developer/XCTestDevices` | 0 B（跑 UI 测试后可达数 GB） | XCTest 设备克隆，每次测试会话生成，业界知名膨胀项 | 🟢 L2 |
| `Library/Developer/DVTDownloads` | 0 B（会增长） | Xcode 组件下载缓存 | 🟢 L2 |

本期新增可回收（本机即时值）：约 85 MB；价值主要在 XCTestDevices / Temp / DVTDownloads 的**长期增长**与 DerivedData 的日常回流（每次构建都会再长回来）。

## 2. 关键判定

### 2.1 DerivedData 是 L2 不是 L3

对标 DevCleaner / CacheCleaner：DerivedData 均为 Safe 类。内容只有构建产物、模块索引、日志，全部由源码重新编译再生，没有任何用户状态。L2 一键清理。

### 2.2 Devices 不重复登记

`simulator`（`CoreSimulator/Devices`，L3）已存在，级别语义正确（重置语义：已装 App 与沙盒数据清除，Xcode/simctl 按需重建）。本期只在测试里把它固化进红线组，不改条目。

### 2.3 模拟器旧运行时（iOS 17.5 等）路径够不着也无需处理

本机 `Profiles/Runtimes` 已空：现代 Xcode 的运行时走系统级 cryptex（`/Library/Developer/CoreSimulator/Volumes`，在 home 之外）。0.8.3 登记的 `simulator-runtimes` 条目继续兜住用户手动安装的 `.simruntime`。RuntimeMap.plist 是文件，机制不收。

## 3. 红线清单（测试固化）

1. DerivedData / Temp / DeveloperDiskImages / XCTestDevices / DVTDownloads 必须 L2（可再生无状态），不许降级或升格。
2. CoreSimulator 下**禁止登记任何文件路径**（`RuntimeMap.plist` / `device.plist` / `device_set.plist`）与 `Devices/<UUID>` 级别的具体设备（设备粒度交给既有 L3 整目录 + 用户确认，机制不猜 UUID）。
3. `Library/Developer/Xcode` 下禁止登记 `UserData` 根（含 KeyBindings / IDEPreferencesController.xcuserstate 等用户个性化状态）；已有的 `xcode-previews` / `xcode-ibsupport` 指向其子目录，维持不变。
4. 全部新条目 `Os::MacOnly`（Xcode / CoreSimulator 在 Windows 不存在）。

## 4. 不做（本期明确排除）

- `CoreSimulator/Devices/<UUID>` 单设备粒度判定（区分「未用过 / 常用」需要 simctl 状态采集，超出白名单纯路径机制；既有 `simulator` L3 整目录已覆盖需求）。
- `/Library/Developer/CoreSimulator/Volumes`（系统级 cryptex 运行时，非用户域，需要特权且 Apple 自管生命周期）。
- `Instruments` 脚本模板缓存等亚目录：体量小、无实测数据，不编造路径。
- Windows 侧无对应物（Xcode 不跨平台）。

## 5. 交付物与验收

1. `whitelist.rs`：5 条登记（Xcode 组 3 + CoreSimulator 组 1 + Developer 组 1）+ §3 红线测试 + 存在性/级别测试
2. `src/i18n/messages/namespaces/disk.ts`：zh / en 各 5 个 `disk.cache.<key>`
3. CHANGELOG 0.8.5 段补 M28（与 M27 同版本并行）、README 路线图 M28 行、roadmap 文档状态修正
4. 门禁：cargo test / clippy -D warnings / fmt / tsc / pnpm build（含 check-i18n）
