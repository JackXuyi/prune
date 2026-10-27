# Prune · A4/A5 明细验收记录

> 2026-10-27 · 方案：`docs/a4a5-detail-plan.md` · 探针：`cargo run --release -p prune-core --example a45-probe`

## 1. 单测（cargo test --workspace 全绿）

| 断言 | 结果 |
|---|---|
| A5 明细仅限 simulator/playwright/cypress 三 key，按体积降序，age 取自指纹 mtime | ✅ `details_only_for_a5_keys_and_sorted_by_bytes` |
| device.plist 解析出「设备名 · 运行时」；损坏/缺失 plist 回退不报错 | ✅ `simulator_note_parses_device_plist_and_falls_back` |
| 运行时串美化（`iOS-17-5` → `iOS 17.5`，不认识的原样返回） | ✅ `runtime_is_prettified` |
| A4 明细 home 相对路径 + age 正确 | ✅ `node_module_details_names_are_home_relative` |
| 旧报告 JSON（无 details 字段）反序列化不炸 | ✅ `cache_group_deserializes_without_details` |
| preview：有明细的 L3 组逐项且 needs_confirm，home 本体绝不进清单；无明细维持旧行为 | ✅ `preview_uses_details_for_l3_groups` / `preview_without_details_keeps_statistics_note` |
| execute：node_modules 无授权拒绝；**有授权但目录名 ≠ node_modules 仍拒绝**；授权齐全放行到 trash 阶段 | ✅ `execute_rejects_node_modules_without_report_allowlist` / `execute_rejects_non_node_modules_even_if_allowlisted` / `execute_allows_reported_node_modules` |

计数：prune-core **101**（+10）+ prune-app 7 + prune-cli 15 = **123 项全绿**。

> 实施备注：`execute_allows_reported_node_modules` 在 CI/临时目录（`/var/folders`）下 Finder 拒绝入废纸篓
> （macOS 限制），测试断言「错误发生在 trash 阶段而非护栏阶段」；真实路径的成功清理由真机验收覆盖。

## 2. 真机探针（本机 2026-10-27，全量 17.2 s，明细构建近零成本）

**A4 node_modules**：55 项 / 17.02 GB；Top 3：newfrontend 2.11 GB（27 天）、backend 1.18 GB（13 天）、
Personal-Learning-OS 1.14 GB（5 天）；**12 项超过 180 天**（标灰档）。
dry-run：46 项（9 项 0 字节空目录被过滤）· needs_confirm=true · 合计 17.02 GB · 首项 label 形如
「项目依赖（55 个 node_modules） · Desktop/git/newfrontend/node_modules」。→ 验收 #3/#5 通过

**A5 simulator**：12 台全部解析出可读形态，如
`2.21 GB · 629 天未改动 · iPhone 15 · iOS 17.5（67B3DA13-…）`。→ 验收 #2 通过

**A5 playwright**：10 项（chromium-1179 / chromium-1155 / webkit-2123 / firefox-1471 / ffmpeg-1011…）；
**cypress 未发现**（本机未装，组不出现，符合白名单「存在才进报告」语义）。

## 3. 护栏复核

- node_modules 路径不在静态白名单：`execute` 凭报告授权表放行，硬护栏三连——canonicalize 在 home 内
  （含 macOS `/var → /private/var` 规范化）、**file_name 必须 == node_modules**、授权表精确相等；
  授权表每次调用从当前报告组装（`commands.rs`），报告过期即失效。
- 其余护栏不动：L3 需确认、执行时重算体积走 20 GB 熔断、全程废纸篓。
- pnpm 式 node_modules 内的 store 符号链接：体积口径跳过（不虚报），整项入废纸篓不波及 store 本体。

## 4. 结论

**A4/A5 通过。** node_modules 活跃度（A4）与模拟器/浏览器二进制（A5）明细随全量与增量扫描产出，
逐项 L3 确认通道闭环；`m1-disk-plan.md` D-K4 已回扫为「A7 落地后追加实现」。
