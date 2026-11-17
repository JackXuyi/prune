# D-02 Homebrew 分发方案（tap + cask 双架构 + uninstall）

> 对应 `prune-roadmap-2027-dev-first.md` Wave 1 D-02（4 人日），验收口径：**一台干净 Mac `brew install --cask` 后 `prune scan` 直接可用**。
> 本文的【事实】均已在仓库核过；【估算】为工作量判断。

## 0. 一句话

> 一个 tap 仓库（`JackXuyi/homebrew-prune`），一个 cask（GUI + CLI 一体）、一个 formula（CLI-only），由 release.yml 在每次发版后自动渲染推送——brew 从此是主要安装路径，Releases 页降级为兜底。

## 1. 已核事实（方案的地基）

| # | 事实 | 出处 | 对方案的影响 |
|---|---|---|---|
| F1 | homebrew core **无** `prune` formula/cask【事实，formulae.brew.sh formula+cask 双 404】 | 本方案核验 | 命名自由，`prune` 直接可用；tap 内命名仍加前缀防将来 core 收编同名 |
| F2 | release.yml 已产出双架构 dmg：`Prune_*_aarch64.dmg` / `Prune_*_x64.dmg`（macos-latest 交叉编译，matrix: `aarch64-apple-darwin` / `x86_64-apple-darwin`） | release.yml L204-210 | cask 的 `on_arm`/`on_intel` 块直接指向现有 asset，**不改产物命名** |
| F3 | **CLI 二进制不在任何 release 产物里**：dmg 只装 GUI（tauri bundle targets "all"），README 让 Linux 用户 `cargo build` | tauri.conf.json / README Install | D-02 最大的隐藏工作量：CLI 必须先「有得下载」才能「有得装」 |
| F4 | CLI bin 名为 `prune`（crates/prune-cli `[[bin]] name = "prune"`），GUI 内部走 prune-core，**运行时不依赖 CLI 二进制** | crates/prune-cli/Cargo.toml | CLI 可以作为「搭车产物」塞进 .app，不影响 GUI 行为 |
| F5 | 当前 ad-hoc 签名（D-01 未做），dmg 用户需手动绕 Gatekeeper；**brew cask 下载不经 LaunchServices quarantine，GUI 直接可开** | README Install / Homebrew 机制 | D-02 可先于 D-01 落地且自带体验收益；但 D-01 仍是官网 dmg 路径的门禁，两者不是替代关系 |
| F6 | app 内置 updater（latest.json + minisign，Draft → 发布链路） | release.yml / updater-plan.md | cask 装的 app 被 updater 原地覆盖后，brew 的版本元数据会失真——见 §6 DP-B4 |
| F7 | release 已有的 secrets：APPLE_CERTIFICATE、TAURI_SIGNING_PRIVATE_KEY 等 | release.yml | 需新增 `HOMEBREW_TAP_TOKEN`（GitHub PAT，对 tap 仓库 push 权限） |
| F8 | 存在 gitee 远端，但 Homebrew 用户习惯 GitHub | git remote -v | tap 放 GitHub，gitee 可后续做镜像（DP-B6） |

## 2. 分发拓扑（一张表说清谁装什么）

| 渠道 | 命令 | 装什么 | 给谁用 |
|---|---|---|---|
| **cask `prune`**（主路径） | `brew install --cask jackxuyi/prune/prune` | Prune.app（dmg）+ CLI 二进制（随 .app 分发，cask `binary` stanza 链接到 `$(brew --prefix)/bin/prune`） | 90% 用户：GUI + CLI 一步到位，满足路线图验收「prune scan 直接可用」 |
| **formula `prune-cli`** | `brew install jackxuyi/prune/prune-cli` | 仅 CLI（universal binary tar.gz） | 不想要 GUI 的服务器/极简用户；官方 README 的 M0′「安装路径归真」落点 |
| Releases 页 | 手动下载 dmg | GUI only（现状不变） | 兜底；README 保留但降级为次要路径 |
| Linux | `cargo build --release -p prune-cli` | 源码（现状不变） | Homebrew on Linux 用户也可用 formula，但 Linux 读数能力本就受限，不承诺 |

**为什么 CLI 跟 .app 走而不是独立 pkg**：F4（GUI 不依赖 CLI）+ 单产物原则——cask 的 `binary` stanza 直接链接 `.app/Contents/Resources/bin/prune`，卸载 cask 时 GUI 与 CLI 链接一并消失，不需要第二个包来装 CLI，也不存在 GUI/CLI 版本错位（同一份 dmg 就是同一版 CLI）。

## 3. 构建侧改动（F3 的解法）

### 3.1 CLI 进 .app：`beforeBuildCommand` 链

```
tauri build（per target triple）
  └─ beforeBuildCommand:
       1. cargo build --release -p prune-cli --target <triple>   # 与 GUI 同 target，天然同架构
       2. cp target/<triple>/release/prune src-tauri/resources/bin/prune
       3. pnpm tauri build（resources 配置把 resources/bin/prune 打进 Contents/Resources/bin/）
```

- `bundle.resources` 加 `resources/bin/prune` → 产物落 `Prune.app/Contents/Resources/bin/prune`
- D-01 落地后该二进制随 .app 一起被签名/公证，无需单独处理
- 本地 dev 构建同样受益（beforeBuildCommand 保证文件总是存在，dev 不炸 resources 缺文件）

### 3.2 release assets 新增：CLI universal tar.gz（给 formula 用）

```
release.yml macOS matrix 两 job 各产出 per-arch CLI 二进制
  └─ 新 job `cli-universal`：
       lipo -create aarch64/prune x86_64/prune -output prune   # universal，单文件双架构
       tar.gz → Prune_CLI_<ver>_universal_macos.tar.gz 上传到 Release
```

- formula 用**单 URL + 单 sha256**（universal），不需要 per-arch bottle 或 formula 内分支——CLI 只有几 MB，universal 体积翻倍不构成问题
- formula `bin.install "prune"`，装完即 `prune --version` 可用

### 3.3 新 job `homebrew-tap`（发布后自动推 tap）

```
依赖：release 发布完成（draft → public 之后，tap 不能指向 404）
  ├─ 下载两个 dmg + universal tar.gz，算 sha256
  ├─ 渲染 Casks/prune.rb（版本、双 arch url+sha、binary stanza、zap 清单）
  ├─ 渲染 Formula/prune-cli.rb（version、url、sha、bin.install）
  └─ clone JackXuyi/homebrew-prune → commit（"prune v0.13.2"）→ push（HOMEBREW_TAP_TOKEN）
```

- 渲染脚本住 `scripts/brew-tap.mjs`（与 release.mjs 同风格，Node 无新依赖）
- cask/formula 模板内嵌在脚本里，版本与 sha 由 CI 注入，**人手只写一次模板，此后发版零操作**
- 幂等：同版本重跑 = 覆盖 commit（force push 同 tag 的 cask 文件），与 release.yml 对同版本 Release 的幂等重跑行为对齐

## 4. tap 仓库与命名

- 仓库：`JackXuyi/homebrew-prune`（新建，public）
- cask 文件：`Casks/prune.rb`；formula：`Formula/prune-cli.rb`
- F1 已确认 core 无同名冲突，但为防将来 core 收编通用工具名，cask/formula 均通过 tap 全名安装（`jackxuyi/prune/prune`），不宣传裸 `brew install prune`
- livecheck：cask 加 `livecheck do url :url strategy :github_latest end`，formula 同理——tap 自检不断链

## 5. 卸载（uninstall / zap）

| 动作 | 行为 | 说明 |
|---|---|---|
| `brew uninstall --cask prune` | 删 .app + `$(brew --prefix)/bin/prune` 链接 | 默认行为，**不碰任何用户数据** |
| `brew uninstall --zap --cask prune` | 额外清：`~/Library/Application Support/com.xuyi.prune`、`~/Library/Caches/com.xuyi.prune`、`~/Library/Logs/com.xuyi.prune`、`~/Library/Preferences/com.xuyi.prune.plist` | zap 是用户显式动作；清单**逐条列全路径，不用 wildcard**（与 Prune「不自动删、删必可预期」的底线同源） |
| `brew uninstall prune-cli` | 删 bin 链接 | formula 无数据目录 |

## 6. 决策点（默认值即建议，不反对即按默认走）

| # | 决策 | 选项 | 默认 | 理由 |
|---|---|---|---|---|
| DP-B1 | tap 仓库名 | `homebrew-prune` / `homebrew-tap` | **homebrew-prune** | 安装命令自解释（`jackxuyi/prune/...`），将来加 second cask 不违和 |
| DP-B2 | CLI 分发方式 | 随 .app + binary stanza / 独立 pkg 装 CLI | **随 .app** | 单产物、版本零错位、卸载一次干净（§2） |
| DP-B3 | formula 架构 | universal 单 URL / per-arch 两条 | **universal** | CLI 小，universal 换 formula 模板最简 |
| DP-B4 | updater 与 brew 双通道 | a) 先不动 updater 代码，cask + README 写明 b) updater 感知 brew 安装（.app 内标记文件）并改提示 | **a** | b) 要改 updater.rs——与未合流的 updater 工作流冲突，且改提示逻辑需设计；先文档说清「brew 用户升级走 `brew upgrade --cask`」，updater 侧感知合流后单独排期 |
| DP-B5 | zap 清单 | 提供 / 不提供 | **提供** | zap 只在 `--zap` 显式触发；提供精确清单是诚实，wildcard 才是危险 |
| DP-B6 | tap 托管 | GitHub / gitee | **GitHub** | Homebrew 生态惯例（`brew install user/repo/name` 语法即 GitHub 约定）；gitee 后续可镜像 |
| DP-B7 | 与 D-01 的顺序 | 先 D-01 / 先 D-02 / 并行 | **D-02 可先做** | F5：brew 路径不碰 Gatekeeper，先上先收益；dmg 路径的公证（D-01）独立进行，两者互不阻塞 |

## 7. 工作量【估算】

| 项 | 人日 |
|---|---|
| 构建侧：beforeBuildCommand 链 + resources 配置 + local dev 验证 | 0.5 |
| release.yml：CLI per-arch 编译 + lipo universal + 上传 asset | 0.5 |
| `scripts/brew-tap.mjs`：cask/formula 模板渲染 + sha 注入 | 1.0 |
| release.yml 新 job：渲染 + 推 tap（含 token、幂等重跑） | 1.0 |
| 卸载/zap 清单 + README Install 段重排（brew 为主） | 0.5 |
| 干净 Mac 实测（VM/快照）+ `brew style`/`brew audit` + 双通道更新冲突实测 | 1.0 |
| **合计** | **4.0**（对齐路线图 D-02） |

版本落点：跟随下一个 CLI 版本号发布（`0.13.2` 或 `0.14.0`），不单独发版。

## 8. 验收标准（可判定）

| # | 验收 | 判定方式 |
|---|---|---|
| A1 | 干净 Mac `brew install --cask jackxuyi/prune/prune` 后 `prune scan` 直接可用 | VM/快照实测，路线图原始口径 |
| A2 | 双架构各自可用 | Intel Mac（或 x64 VM）装 cask 后 `prune --version` 正常；arm 侧 `file` 输出 arm64 |
| A3 | formula 独立可用 | `brew install jackxuyi/prune/prune-cli` 后 `prune doctor --json` 出契约 JSON |
| A4 | 卸载干净 | `brew uninstall --cask prune` 后 `which prune` 为空、/Applications 无 Prune.app；`--zap` 后 Application Support 对应目录消失 |
| A5 | tap 门禁 | `brew style --cask` / `brew audit --cask`（在线模式）零 error |
| A6 | 发版自动化 | 打 tag → Release 发布 → 5 分钟内 tap commit 自动出现，cask version 与 tag 一致 |
| A7 | 幂等 | 同 tag 重跑 release.yml，tap 无重复 commit、cask 内容一致 |

## 9. 风险登记册

| # | 风险 | 概率 | 影响 | 值 | 应对 |
|---|---|---|---|---|---|
| RK-B2-1 | updater 原地覆盖 .app 导致 brew 元数据失真（`brew upgrade` 误判） | 4 | 3 | 12 中 | DP-B4 默认项：文档写明二选一；updater 感知 brew 排期在 updater 工作流合流后 |
| RK-B2-2 | tap push token 泄露 | 1 | 4 | 4 低 | fine-grained PAT 仅限 tap 仓库 contents:write；token 不进日志（渲染脚本不 echo） |
| RK-B2-3 | CLI 塞进 .app 后 bundle 校验/签名异常 | 2 | 3 | 6 低 | resources 是 Tauri 标准机制；D-01 落地时 codesign 全 bundle 一并覆盖，验收时 `codesign -vv` 实测 |
| RK-B2-4 | core 未来收编同名 `prune` formula | 1 | 2 | 2 低 | 安装命令始终带 tap 全名；真发生了再改名，tap 侧成本半天 |
| RK-B2-5 | Release 是 draft 期间 tap 指向 404 | 2 | 3 | 6 低 | homebrew job 依赖发布完成事件（release published），不挂在 build job 后 |

## 10. 与其他工作的依赖关系

| 对象 | 关系 |
|---|---|
| D-01 签名公证 | **无阻塞**（F5）；D-01 落地后 cask/formula 零改动，仅 dmg 路径体验提升 |
| D-14 产品化（`prune report` 子命令） | 无阻塞；但若 D-14 先落地，cask 分发的 CLI 自带 report——建议 D-02 先上（分发先通），D-14 跟下个版本走 |
| updater 工作流（未合流） | DP-B4 默认不碰其代码；`HOMEBREW_TAP_TOKEN` 入 secrets 时机与 updater 的 secrets 管理同页管理 |
| D-06 文档 | README Install 段重排并入本方案（0.5 人日内），`check:docs` 门禁同步覆盖 brew 命令 |
| M0′（安装路径归真） | 本方案即其落地载体：README 的 brew 段落从「计划」变「事实」 |

## 11. 这份方案不做什么

- 不做 Windows winget / Linux PPA（Wave 1 范围外；Windows 走 installer，README 已交代）
- 不做 bottle（官方托管预编译）——universal tar.gz 已覆盖双架构，bottle 是规模后的优化
- 不做首页提交（homebrew-core 收编）——有 300 安装基线（G1 门禁）之后再议
- 不改 updater 代码（DP-B4 默认项把冲突隔离在外）

## 12. 实施记录（2026-11-17）

### 12.1 落点清单

| 文件 | 改动 |
|---|---|
| `scripts/prepare-cli-resource.mjs`（新增） | triple 解析（`CLI_TARGET_TRIPLE` → `TAURI_ENV_TARGET_TRIPLE` → `rustc -vV` host）→ `cargo build --release -p prune-cli --target` → 拷到 `src-tauri/resources/bin/prune`。脚本按 `import.meta.url` 定位仓库根，不受 beforeBuildCommand 的 cwd 影响 |
| `src-tauri/tauri.macos.conf.json`（新增） | 平台 overlay：`beforeBuildCommand` 改成 `node scripts/prepare-cli-resource.mjs && pnpm build`；`bundle.resources` 加 `resources/bin/prune` → `bin/prune`（落 `Contents/Resources/bin/prune`）。**只在 macOS 构建路径生效**，Windows/Linux 产物与历史版本一致 |
| `.gitignore` | 忽略 `src-tauri/resources/bin/`（构建期拷贝产物） |
| `.github/workflows/release.yml` | ① bundle 的 tauri-action step 加 `CLI_TARGET_TRIPLE=${{ matrix.target }}`；② macOS 腿上传 `cli-<target>` artifact；③ 新 job `cli-universal`（lipo → tar.gz → 传 draft release，同名先删保证幂等）；④ `release-notes` 依赖改为 `[bundle, cli-universal]`；⑤ 新 job `homebrew-tap`（`needs: release-notes`，仅 tag 触发）；⑥ Release 下载表加 CLI 行 |
| `scripts/brew-tap.mjs`（新增） | 资产发现 → sha256 注入 → 渲染 cask/formula → `--dry-run` 落 `output/brew-tap/`，否则 clone tap 仓库、幂等判断、commit+push |
| `README.md` | Install 段重排：brew 为主（cask / formula 两条命令 + uninstall/zap 说明），直接下载降为兜底；「Open Anyway」段明确只作用于直接下载 |

### 12.2 本地已验收

| 项 | 结果 |
|---|---|
| overlay 合并与资源落地 | `npx tauri build --bundles app` 成功；`Prune.app/Contents/Resources/bin/prune` 存在、`Mach-O 64-bit executable arm64`、`prune v0.13.1`，与 `tauri.conf.json` 的 GUI 版本一致 |
| tar 结构 | 按 CI 同法本地打包：`tar tzf` 只有一项 `prune`，解包后可直接执行（`bin.install "prune"` 成立） |
| cask sha 正确性 | 渲染出的 arm/intel sha 与 GitHub API `digest` 逐位一致（`863e25ec…` / `335564c9…`） |
| 模板语法 | `ruby -c` 两份文件均 Syntax OK |
| workflow 结构 | YAML 解析通过，依赖链 `bundle → cli-universal → release-notes → homebrew-tap` |

### 12.3 实施期发现的四个偏差（都已修）

1. **资产名带 `v` 前缀会让 formula URL 404。** `github.ref_name` 是 `v0.13.1`，而 dmg 与 formula 的 `#{version}` 都是不带 `v` 的 `0.13.1` → 统一用 `${REF_NAME#v}` 派生。
2. **tar 多包一层目录会让 `bin.install "prune"` 直接报找不到文件。** 改成 tar 根目录只放 `prune`。
3. **`TAURI_ENV_TARGET_TRIPLE` 不能单独依赖。** cross 编译（arm64 host 出 x64 包）时它若缺位，脚本会退回 host triple，把 arm64 二进制塞进 x64 的 dmg；改为 workflow 显式注入 `CLI_TARGET_TRIPLE`。
4. **asset sha 不再下载实算。** GitHub API 已返回 `digest`（sha256），改为优先取它、缺失时才 curl 落盘算——本地受限网络下原实现会 100s 超时（实测 ETIMEDOUT），CI 里也省掉 13MB 下载。

### 12.4 验收状态

| # | 状态 | 说明 |
|---|---|---|
| A1 A2 A3 A4 | ⏳ 待发版 | 需要 tap 仓库与一个真实 Release 才能实测 |
| A5 | ⚠ 部分 | `ruby -c` 通过；`brew style --cask` 在本机沙箱被 SIGTERM（brew 需写 Homebrew 前缀且要联网取 rubocop），需在本机终端或 CI 手跑一次 |
| A6 A7 | ⏳ 待发版 | job 已接线，幂等由「同名 asset 先删」+「tap 内容 diff 为空则跳过 commit」两处保证 |

### 12.5 发版前的一次性人工前置（脚本会把缺失项直接报出来）

1. 建公开仓库 `JackXuyi/homebrew-prune`（当前 404），`main` 分支内预置 `Casks/`、`Formula/` 两个目录；
2. 该仓库上建 fine-grained PAT（仅 Contents: Read and write）→ 存为仓库 secret `HOMEBREW_TAP_TOKEN`；
3. 抬版本号发版（`0.13.2` 起），首个带 CLI universal asset 的版本才会让 `homebrew-tap` job 通过。

