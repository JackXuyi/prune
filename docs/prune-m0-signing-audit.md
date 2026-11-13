# M0 分发产物签名核查（v0.9.3）

> 核查日期：2026-11-13 17:05–17:12 GMT+8  
> 核查对象：`Prune_0.9.3_aarch64.dmg`（GitHub Release v0.9.3，published 2026-11-13T08:55:09Z）  
> 核查人：主理人（产品舵手）在本地 macOS 15.7.7 / arm64 上实测  
> 触发依据：`docs/prune-roadmap-2027h1.md` §3 M0（Q1 第 0 周条件项）

---

## 一、结论

**判定：`ad-hoc` 签名 + 未公证。**

> **后续决策（2026-11-13，见 §11）：产品负责人决定本两季度不购年费、不签名/公证。** 因此本文 §7 的"修复路径"与 §10 的第 1 条**不作为执行计划**，改为 **M0′ 安装路径归真**（0.5 人日，随 v0.9.4）承接。本文件的实测结论不受该决策影响——它记录的是产物的客观状态。

| 项 | 实测结果 | 判定 |
|---|---|---|
| `.app` 签名的代码目录标志 | `flags=0x10002(adhoc,runtime)`、`Signature=adhoc` | ad-hoc 签名 |
| 签名团队标识 | `TeamIdentifier=not set` | 无开发者身份 |
| 签名自洽性（`--verify --deep --strict`） | `valid on disk` / `satisfies its Designated Requirement` | 自洽（ad-hoc 也能自洽，**不等于 Gatekeeper 认可**） |
| **Gatekeeper 评估（`spctl -a -vv -t exec`）** | **`rejected`** | **系统拒绝放行** |
| 公证票据（`stapler validate`） | `does not have a ticket stapled to it` | 未公证 |
| `.dmg` 容器签名 | `code object is not signed at all` | 容器未签名 |

一句话：**产物本身完整、可校验、能运行，但对 Gatekeeper 而言是"来历不明的软件"。用户从浏览器下载后双击，会被拦一次，必须手动绕过。**

---

## 二、核查方法与环境

- 环境：macOS 15.7.7，`spctl` / `codesign` / `xcrun stapler` 均为系统自带工具
- 产物获取：`curl --http1.1 -L` 从 GitHub Release 直链下载（**未走浏览器，因此该文件不带 `com.apple.quarantine` 属性**；真实用户经浏览器/Safari 下载会带该属性，拦截强度只会更高）
- 挂载：`hdiutil attach -nobrowse -readonly -mountpoint <dir>`
- 下载物校验：`hdiutil verify` → `checksum of "a.dmg" is VALID`（说明不是下载损坏导致的假阴性）
- 已记录 SHA256：`76b4eadbedff7982c48dc02dcb981e8712515ae6a2595f3a1626248c5e3e882e`（5,941,130 bytes，与 Release API 声明大小一致）

---

## 三、原始证据（逐条可复现）

### 3.1 `.app` 签名详情（关键行）

```
Executable=.../Prune.app/Contents/MacOS/prune-app
Identifier=com.xuyi.prune
CodeDirectory v=20500 size=25551 flags=0x10002(adhoc,runtime) hashes=792+3 location=embedded
CandidateCDHash sha256=4ed444c0af82e6499cceb8c528df83f910532401
Signature=adhoc
TeamIdentifier=not set
Runtime Version=26.5.0
Internal requirements count=0 size=12
```

三条要点：
1. `flags=0x10002(adhoc,runtime)` —— 硬运行时（hardened runtime）已开，但签名是 ad-hoc。
2. `Signature=adhoc` —— 没有 Developer ID 证书链。
3. `TeamIdentifier=not set` —— 无团队标识，Gatekeeper 无法把它关联到任何已注册开发者。

### 3.2 自洽性 vs Gatekeeper（这两条必须分开看）

```
$ codesign --verify --deep --strict --verbose=2 Prune.app
Prune.app: valid on disk
Prune.app: satisfies its Designated Requirement      ← 自洽通过

$ spctl -a -vv -t exec Prune.app
Prune.app: rejected                                  ← 系统拒绝
```

**这是本次核查最重要的方法论点**：`codesign --verify` 通过只说明"签名没坏"，`spctl` 拒绝才说明"系统不信"。只看前者会得出完全错误的结论——很多 ad-hoc 包都能通过 `--verify`。

### 3.3 公证票据

```
$ xcrun stapler validate Prune.app
Prune.app does not have a ticket stapled to it.
```

### 3.4 `.dmg` 容器

```
$ codesign -dv --verbose=4 Prune_0.9.3_aarch64.dmg
a.dmg: code object is not signed at all
```

若走正式签名 + 公证，`tauri-bundler` 会给 dmg 一并签名。容器未签名是"整条链路都没配"的旁证。

---

## 四、代码侧佐证（为什么是 ad-hoc）

` .github/workflows/release.yml:260-263`：

```bash
if [[ -z "${CERT_B64}" || -z "${CERT_PASSWORD}" || -z "${SIGN_IDENTITY}" ]]; then
  echo "APPLE_SIGNING_IDENTITY=-" >> "$GITHUB_ENV"
  echo "mode=adhoc" >> "$GITHUB_OUTPUT"
  echo "::warning::签名三件套不全 → 使用 ad-hoc 签名。产物未公证，用户首次打开需右键 → 打开（或 xattr -cr /Applications/Prune.app）"
```

`.github/workflows/release.yml:287`（公证分支）：

```bash
echo "::notice::APPLE_ID / APPLE_PASSWORD / APPLE_TEAM_ID 未配全 → 跳过公证"
```

即：**签名三件套（证书 / 证书密码 / identity）任一为空 → 主动降级 ad-hoc；公证三件套不全 → 跳过公证。两条路径都只打 warning/notice，CI 依然 success。**

Actions 侧佐证（run `43389501341`，v0.9.3，conclusion=success）：macOS 两个 job 均含步骤 `Resolve macOS signing mode`，**无独立 notarize 步骤**，整个 job 仍然通过。

---

## 五、用户实际会遇到什么

【推断，基于 ad-hoc + quarantine 的 macOS 行为，本机未模拟浏览器下载路径】

1. Safari/Chrome 下载 dmg → 文件带 `com.apple.quarantine`
2. 双击挂载 → 拖到 Applications → 双击启动
3. **Gatekeeper 弹窗拦截**："Apple 无法验证 'Prune' 是否包含可能危害 Mac 的恶意软件" / "无法打开，因为无法验证开发者"
4. 用户需自行完成以下之一：
   - 系统设置 → 隐私与安全性 → 底部"仍要打开"
   - 终端 `xattr -dr com.apple.quarantine /Applications/Prune.app`
   - （新版 macOS 上"右键→打开"已不再对所有情况奏效）

**这条路对开发者的摩擦远大于对普通用户**——目标用户正是最熟悉 `xattr` 的那批人，所以流失的不会是全部，但**放弃率不可忽略且完全不可测**（放弃者不会留下任何痕迹）。

---

## 六、对指标与门禁的影响（路线图需承接的部分）

| 受影响项 | 机制 | 后果 |
|---|---|---|
| **D1（下载数）→ 真实装机** | 下载了但被拦/放弃的人，D1 记了、装机没有 | r₂（安装成功率）是门禁 A① 最大单点误差源；此时 r₂ 可能远离假设区间 0.5–0.8 的下端 |
| **D2 ÷ D1（安装链健康度）** | 下载多、启动少 → 比值显著 <0.4（路线图定的红灯） | **这个红灯会亮，但它亮的原因不是"没人用"，而是"打不开"**——若不先修签名就读这个数，会把分发缺陷误读成需求不足 |
| **门禁 A① 装机 ≥3,000** | 门槛本依赖 r₂ | 在 r₂ 未知且偏低时，3,000 这个门槛的方向是**保守的**（真实装机只会比 D1 更低），但**结论会是"不该商业化"，而真实原因可能是"装不上"** |
| **口碑层（Issue 数）** | 拦截弹窗是高频吐槽点 | 预期 Issue 里会出现"无法打开/已损坏"类问题，可作为旁证 |

**一句话**：这个缺陷不会让产品"少赚钱"，它会让产品**看起来没人要**。在零遥测体系里，这两件事在数据上长得一模一样。

---

## 七、修复路径与成本【估算】

| 步骤 | 内容 | 成本 |
|---|---|---|
| 1 | Apple Developer Program 会员（Developer ID Application 证书的前提） | 年费（美国区 $99/年【事实·公开定价，需按所在地区核实】），审核 1–3 天 |
| 2 | 本机生成 `.p12`（证书 + 私钥），`openssl base64 -A` 出单行 base64 | 0.5 人日（含踩坑；workflow 已内置 p12 合法性预检，能挡住多数配错） |
| 3 | 配 6 个 secrets：`APPLE_CERTIFICATE` / `APPLE_CERTIFICATE_PASSWORD` / `APPLE_SIGNING_IDENTITY` / `APPLE_ID` / `APPLE_PASSWORD`（app-specific password）/ `APPLE_TEAM_ID` | 0.5 人日 |
| 4 | 重发一版，`spctl` 复验 + `stapler validate` 复验 | 0.5 人日（含构建） |
| 合计 | — | **1.5 人日 + 年费**，**不含 Apple 审核等待** |

workflow 侧不需要改代码——它已经写好了两条分支，只差 secrets。这是这次核查里唯一的好消息。

**风险点**：公证是异步的（提交 notarytool 后等结果），首次配置常因 entitlements 或 app-specific password 失效而失败；`release.yml` 里公证失败只打 notice 不红，**修完必须用 `spctl` 实测复验，不能看 CI 绿就认为成了**。

---

## 八、未实测事项（诚实清单）

1. **`.dmg` 在真实 quarantine 环境下的弹窗文案**：本机下载未带 quarantine 属性，未模拟浏览器路径。结论依据 `spctl` 拒绝 + macOS 通行行为，标【推断】。
2. **`x64`（Intel）dmg**：未单独下载实测。同一 workflow、同一 secrets 状态，推定同为 ad-hoc【推断】。
3. **Windows 产物**：`.msi` / `.exe` 无 Authenticode 签名（workflow 无 Windows 签名步骤），未实测，仅由代码推断【推断】。Windows SmartScreen 的摩擦性质与 macOS 不同，未纳入本次结论。
4. **自动更新链路在 ad-hoc 下的行为**：Tauri updater 的签名校验与产物签名是两套机制（updater 用 `.sig` 文件 + 公钥，与 Apple 签名无关），故**自动更新应能正常工作**；但 macOS 每次替换 app 后 Gatekeeper 是否重新评估，未实测。标【假设】。
   - 【事实·补测 2026-11-13】`Prune_aarch64.app.tar.gz`（updater 产物，SHA-256 `1f04579f633e3b584a87a964d69436fa21e9585ef04d7ad017bffe5921009b15`，6,082,227 B 与 Release API 一致）已解包实测：`codesign -dv` 同为 `flags=0x10002(adhoc,runtime)` / `Signature=adhoc` / `TeamIdentifier=not set`，CDHash 与 dmg 内 app **逐字一致**（`4ed444c0…`）——同一 CI run 的同一 bundle；`codesign --verify --strict` 自洽通过（exit 0），`spctl -a -vv` 同样 `rejected`。结论：updater 产物与 dmg 产物签名状态完全同源，§11.3「updater 验签独立不受影响」的判断维持，但从推断升为实测。
5. **真实放弃率**：不可测。这正是 D2÷D1 存在的理由。

---

## 九、复现命令（供审计者重跑）

```bash
cd /tmp && mkdir -p m0 && cd m0
curl --http1.1 -L -o p.dmg \
  "https://github.com/JackXuyi/prune/releases/download/v0.9.3/Prune_0.9.3_aarch64.dmg"
shasum -a 256 p.dmg          # 应为 76b4eadb…
hdiutil verify p.dmg         # 应 VALID
mkdir mnt && hdiutil attach -nobrowse -readonly -mountpoint "$PWD/mnt" p.dmg
codesign -dv --verbose=4 mnt/Prune.app 2>&1 | grep -E "Signature|TeamIdentifier|flags"
spctl -a -vv -t exec mnt/Prune.app       # 应 rejected
xcrun stapler validate mnt/Prune.app     # 应无票据
codesign -dv mnt/../p.dmg 2>&1 | head -2
hdiutil detach mnt
```

---

## 十、给路线图的三个动作（均已执行，2026-11-13）

1. **M0 判定写入**：`ad-hoc` 已写入路线图，但**公证条件项经用户决策撤销**（见 §11），替代为 M0′ 安装路径归真。
2. **D2÷D1 已降级**：从"安装链健康度指标"降为"仅趋势观察"，且 D1 不再作装机体量判定口径；装机代理切到 D2。写入路线图 §5.1 与 §6 门禁 A①。
3. **RK-9 状态**：从"预警"改为"已受控接受"，并附 4 条回头信号。

---

## 十一、决策记录：本两季度不签名、不购 Apple Developer 年费

> 决策人：产品负责人 ｜ 时间：2026-11-13 ｜ 依据：本文件 §1–§8 的实测结论

### 11.1 决策内容

不购买 Apple Developer Program 年费，不申请 Developer ID 证书，不做签名与公证。**v0.9.3 及后续版本的产物保持 ad-hoc 签名状态。**

### 11.2 这个决策的合理之处（不是省钱的借口）

1. **年费买不到"确定性"，只买到"少一次弹窗"。** 即使公证，用户仍要走"下载 → 拖入 Applications"；公证省掉的只是那一次"仍要打开"。对目标用户（开发者）而言，这道摩擦是**熟悉的、可自助解决的**，不是"不明软件不敢装"的普通消费者场景。
2. **它把一笔订阅式承诺，换成了可逆的工程项。** 年费是年年续的；M0′（安装说明）是一次性 0.5 人日，且**即使将来签了名，安装说明仍然是必需品**——签了名也要告诉用户拖到哪。所以 M0′ 不是替代品的妥协，是原本就该做的事。
3. **它避免了一个更隐蔽的依赖**：一旦依赖 Apple 的证书链，分发就多了一个不可控的第三方（审核、证书过期、账号风险）。零遥测、零账号的产品叙事，与"要向 Apple 注册一个开发者账号"之间存在张力。

### 11.3 代价（必须记账，不能自我安慰）

| 代价 | 严重度 | 已做的对冲 |
|---|---|---|
| **r₂（安装成功率）永久不可测**，D1 与真实装机之间的断层无法分离 | **高** | 装机代理切到 D2（D2 已穿过 Gatekeeper）；D1 降为"分发触达"参考值 |
| **门禁 A① 原口径（D1 ≥3,000）失效** | 高 | 改为"任一周 D2 ≥800"，且不再依赖 r₁/r₂ 假设 |
| **访谈样本被二次污染**：愿意绕过 Gatekeeper 的人，本身摩擦耐受度更高 → p 与触达率都会被高估 | 中 | 引用纪律里追加：访谈读数只能作**上界**用（原已规定只能走下界/区间，方向一致） |
| **安装失败者的声音完全听不到**：他们既不留数据，也不来访谈，也不提 Issue | **高** | 只能靠 M0′ 上线前后的 D2÷D1 相对变化间接观察（假设 A7） |
| 若未来商业化，**付费用户装不上 = 灾难** | 决定性 | RK-9 回头信号④：任何商业化动作前必须先签名，无例外 |

**一句话总结代价**：这个决定让产品在数据上变得**更不可解释**，但换来了**更诚实的指标结构**——原来的 D1 口径看似精确，实际是把两种完全不同的失败（打不开 / 不想要）混在一个数里。

### 11.4 回头信号（可逆，不建议死守）

出现任一，重新评估年费（成本仍只有 1.5 人日 + 年费，workflow 已备好双分支，只差 6 个 secrets）：

1. **本周 D1 ≥ 500 且 D2÷D1 < 0.35 连续 4 周** —— 说明下载发生了、启动没发生，摩擦是主瓶颈，此时 $99 的回报率最高。
2. **Issue / 讨论区出现 ≥3 条「无法打开 / 已损坏 / 无法验证开发者」** —— 用户主动抱怨即代表沉默放弃者数量可观。
3. **Q2 死亡线落在「不可判定」而不是「判负」** —— 意味着样本凑不够，而样本凑不够的原因里，安装摩擦是唯一可修的一项。
4. **任何商业化动作之前（硬前置，无例外）** —— 付费用户装不上比免费用户装不上严重一个数量级。

### 11.5 这个决策改变不了的一件事

**它不改变"v0.9.3 的包现在会被 Gatekeeper 拒绝"这个事实。** 用户下载后遇到的第一个界面仍然是拦截弹窗。所以 M0′ 的安装说明不是"补偿"，是**当前唯一的止损手段**——而且它现在比签名更紧急，因为签名要等证书审核，说明文档今天就能写。
