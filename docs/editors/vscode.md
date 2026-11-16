# VS Code 配方：把 Prune 读数接进任务面板

> W2/D-12 编辑器配方包。Prune 不写任何 `.vscode/` 文件——下面是**你粘贴**的片段，
> 与「Prune 不碰用户仓库」同一纪律。命令与本版 CLI usage 逐字一致
> （由 `pnpm check:docs` 门禁检查 C 自动对账）。

## 前提：CLI 可用

```sh
cargo build --release -p prune-cli
ln -sf "$PWD/target/release/prune" ~/.local/bin/prune   # 确保 ~/.local/bin 在 PATH 里
prune --version
```

`prune --version` 不报错即就绪。下面三条任务的输出都是 JSON（`--json`），
适合接给其他工具；想在面板里看人类可读的中文摘要，去掉 `--json` 即可。

## `.vscode/tasks.json` 片段（3 个任务）

粘进项目的 `.vscode/tasks.json`（没有就新建）：

```json
{
  "version": "2.0.0",
  "tasks": [
    {
      "label": "prune: 端口冲突",
      "type": "shell",
      "command": "prune port --conflicts --json",
      "problemMatcher": [],
      "presentation": { "reveal": "always", "panel": "dedicated" }
    },
    {
      "label": "prune: 项目依赖账本",
      "type": "shell",
      "command": "prune projects --json",
      "problemMatcher": [],
      "presentation": { "reveal": "always", "panel": "dedicated" }
    },
    {
      "label": "prune: 一轮读数",
      "type": "shell",
      "command": "prune watch --once --json",
      "problemMatcher": [],
      "presentation": { "reveal": "always", "panel": "dedicated" }
    }
  ]
}
```

三个任务与方案 §9.2 的对应关系（一处偏差，已记录）：

| 任务 | 方案原文 | 本版实际命令 | 为什么 |
|---|---|---|---|
| 端口冲突 | `port` | `prune port --conflicts --json` | 一致；D-07 落地了 `--conflicts` |
| 读数体检 | `doctor --json` | `prune watch --once --json` | **`prune doctor` 尚未实现**；`watch --once` 一轮拉全 cpu/mem/port/disk 读数，是最接近的替代。`doctor` 留给后续批次 |
| 依赖账本 | `projects --json` | `prune projects --json` | 一致 |

跑法：`Terminal → Run Task…` 或 `⇧⌘P → Tasks: Run Task`。

## 可选：`settings.json` 的 `terminal.integrated.env`

如果 `prune` 不在 PATH 里，可以在工作区设置里只为本项目补 PATH
（不要写死机器绝对路径，用 `${env:PATH}` 拼接）：

```json
{
  "terminal.integrated.env.osx": {
    "PATH": "${env:PATH}:/Users/你/.local/bin"
  }
}
```

Windows 用 `terminal.integrated.env.windows`，Linux 用
`terminal.integrated.env.linux`，写法相同。注意：片段里的
`/Users/你` 只是占位——门禁要求配方不含写死的真实用户名，粘贴时替换成自己的。

## 退出码约定

| 码 | 含义 |
|---|---|
| 0 | 正常 |
| 2 | 用法错（参数拼错） |
| 3 | 权限不足 |
| 4 | 探测失败 |
| 10 | `watch` 达到 warn（仅 `--fail-on warn` 时出现） |

VS Code 任务面板对非 0 退出码标红。若想把「warn」也当失败处理，
把第三条任务的命令改成 `prune watch --once --json --fail-on warn`。
