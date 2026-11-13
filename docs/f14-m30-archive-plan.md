# f14 · M30 项目休眠归档（Pro 首个交付）

> 2026-11-13 · roadmap M30：DevCleaner Pro 的独家付费锚点，Prune 用「更窄的语义 + 更硬的护栏」做差异化版本。

## 1. 语义边界（先定清楚，再写代码）

| 问题 | 决定 | 理由 |
|---|---|---|
| 归档什么 | **只归档项目的依赖目录（`node_modules`）**，用户代码/文档/配置一律不碰 | Prune 定位是缓存守门员，不是备份工具。把用户源码打进 tar 再删原件，一旦归档损坏就是数据事故——Docker 类不赌的原则同样适用 |
| 归档到哪 | 受管归档区 `<config>/com.xuyi.prune/archive/<id>.tar.gz` + `manifest.json` | 废纸篓语义之外（废纸篓会被系统清空），可完整还原 |
| 怎么删原件 | 归档写完后**重新打开归档校验**（条目数 + 非压缩字节数必须与扫描一致），通过后才 `trash::delete` 原件 | 「验证后才允许删原件」——roadmap 原话，也是护栏顺序 |
| 怎么恢复 | 冲突检测（目标已存在则拒绝，除非用户确认覆盖→覆盖也走废纸篓）→ 解压 → 校验 → 失败则把解压产物移入废纸篓 | 全程无 `remove_dir_all` 落在用户路径上（沿用 reclaim 的 invariant） |
| dry-run | `plan` / `restore_preview` 独立命令，UI 必须先预览再确认；执行命令一律要求 `confirmed=true` | 与 reclaim 的 dry-run 前置原则一致 |
| 免费/Pro | 本期**不做授权校验**，命令层不埋开关；账本（发现）本就在免费侧 | 商业化切分是 v1.0 的事，先把功能做实；开关留给授权模块上线时接 |

## 2. 流程

### 2.1 归档（创建）

```
plan(root)                     扫描依赖目录：文件数 / 字节数 / 目标归档路径 / id（只读）
create(root, confirmed)        ① 校验依赖目录（必须是 node_modules、非 symlink、绝对路径）
                               ② 流式 tar.gz 写入归档区
                               ③ 校验：重读归档，条目数与字节数必须一致
                                   └ 不一致 → 删自己写的半成品归档，报错，原件未动
                               ④ 写 manifest 条目（先写账）
                               ⑤ trash 原件；失败 → 回滚 manifest + 删归档，报错
```

### 2.2 恢复

```
restore_preview(id)            目标路径 / 文件数 / 字节数 / 冲突标记（只读）
restore(id, confirmed)         ① 归档文件必须存在
                               ② 目标已存在且未确认 → 拒绝；已确认 → 先 trash 目标
                               ③ tar 解压到目标路径（tar crate 默认拒绝绝对路径与 .. 逃逸）
                               ④ 校验解压结果与 manifest 一致；不一致 → 解压产物移入废纸篓 + 报错
```

### 2.3 删除归档（释放归档区空间）

`delete(id, confirmed)`：归档文件走废纸篓 + 移除 manifest 条目。归档区不是只进不出。

## 3. 安全护栏（代码层 + 测试固化）

1. **只归档 `node_modules`**：`validate_deps` 校验目录名、绝对路径、非 symlink——GUI/CLI 传任意路径都会被拒
2. **校验先于删除**：归档字节数与条目数不匹配时原件不动（单测模拟错配）
3. **无 `remove_dir_all` 落在用户路径**：原件与覆盖目标都走 `trash`；只有「自己刚写的归档半成品」允许直接删
4. **恢复目标不可自由指定**：以 manifest 里的 `deps_path` 为准，命令参数只有 `id`
5. **manifest 先写后删**：删除原件失败可回滚，不产生孤儿归档

## 4. 不做（本期明确排除）

- 归档整个项目（含源码）—— 数据事故风险，语义超出定位
- 自动休眠（按天数自动归档）—— 与「人在回路」原则冲突
- 归档加密 / 增量归档 —— DevCleaner 也没做，先不背复杂度
- CLI `prune archive` 子命令 —— 本期 GUI 优先，core 已可复用，后续一行封装

## 5. 交付物与验收

1. `crates/prune-core/src/archive.rs`：plan / create / restore_preview / restore / delete / list + 单测（真实文件系统：归档→校验→恢复往返、冲突拒绝、错配不删原件、非 node_modules 拒绝）
2. `src-tauri/src/commands.rs`：6 条 `archive_*` 命令（执行类要求 `confirmed`）
3. `ProjectHealthCard`：僵尸/偏胖项目行加「归档」入口（预览 → 确认），卡片底部归档区列表（恢复/删除，均先预览）
4. i18n 双语 key；CHANGELOG 0.9.0；README 路线图 M30
5. 门禁：cargo test / clippy -D warnings / fmt / tsc / pnpm build（含 check-i18n）
