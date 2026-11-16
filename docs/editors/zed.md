# Zed 配方：把 Prune 读数接进 tasks

> W2/D-12 编辑器配方包。Prune 不写任何 `.zed/` 文件——下面是**你粘贴**的片段，
> 与「Prune 不碰用户仓库」同一纪律。命令与本版 CLI usage 逐字一致
> （由 `pnpm check:docs` 门禁检查 C 自动对账）。

## 前提：CLI 可用

```sh
cargo build --release -p prune-cli
ln -sf "$PWD/target/release/prune" ~/.local/bin/prune   # 确保 ~/.local/bin 在 PATH 里
prune --version
```

## `.zed/tasks.json` 片段（3 个任务）

粘进项目的 `.zed/tasks.json`（没有就新建）：

```json
[
  {
    "label": "prune: 端口冲突",
    "command": "prune port --conflicts --json",
    "use_new_terminal": false,
    "allow_concurrent_runs": false
  },
  {
    "label": "prune: 项目依赖账本",
    "command": "prune projects --json",
    "use_new_terminal": false,
    "allow_concurrent_runs": false
  },
  {
    "label": "prune: 一轮读数",
    "command": "prune watch --once --json",
    "use_new_terminal": false,
    "allow_concurrent_runs": false
  }
]
```

跑法：`⇧⌘R` 打开 tasks 选择器（`task::Spawn`）。

> 与方案 §9.2 的对应关系（一处偏差，已记录）：方案里的 `doctor --json`
> 因 `prune doctor` 尚未实现，用 `watch --once --json`（一轮拉全
> cpu/mem/port/disk 读数）替代。`doctor` 留给后续批次。

## 变体

- 人类可读的中文摘要比 JSON 更适合终端直读：去掉 `--json` 即可，
  例如 `"command": "prune port --conflicts"`。
- 想让 warn 当失败（CI 场景）：`"command": "prune watch --once --json --fail-on warn"`。

## 退出码约定

| 码 | 含义 |
|---|---|
| 0 | 正常 |
| 2 | 用法错（参数拼错） |
| 3 | 权限不足 |
| 4 | 探测失败 |
| 10 | `watch` 达到 warn（仅 `--fail-on warn` 时出现） |
