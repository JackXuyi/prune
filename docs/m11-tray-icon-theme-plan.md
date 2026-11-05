# M11 · 顶部图标可自定义（DeepSeek 鲸鱼娘 · 吃白米饭看 CPU）

> 目标：菜单栏图标不再写死成剪刀，改成可切换的「图标主题」；第一套自定义皮肤是 DeepSeek 鲸鱼娘，
> 并且她**靠吃白米饭的速度**表达 CPU 忙闲 —— 不是一张静态头像。
> 代码：[`src-tauri/src/tray_icon.rs`](../src-tauri/src/tray_icon.rs)（主题注册表）、[`src-tauri/src/tray.rs`](../src-tauri/src/tray.rs)（菜单 + 动画循环）、[`scripts/gen-tray-whale.mjs`](../scripts/gen-tray-whale.mjs)（素材生成）

## 1. 为什么不能简单换张图

macOS 菜单栏默认走 **template image** 机制：系统只读图标的 alpha 通道，按菜单栏深浅色自己上色。
剪刀图标是纯黑模板图，所以深色菜单栏自动变白。

鲸鱼娘是彩色图，如果沿用 `set_icon_as_template(true)`，彩色会被当作掩码，最终渲染成一块黑白剪影。
因此主题必须自带「是否模板」这个属性：

| 主题 | 资源 | template | 效果 |
|---|---|---|---|
| `scissors` | `icons/tray.png`（黑色剪刀） | 是（macOS） | 随系统深浅色反色 |
| `whale` | `icons/tray-whale/macos.png`（彩色头像 + 饭碗） | **否** | 原色显示 |

## 2. 设计

### 2.1 主题注册表（唯一扩展点）

```rust
pub enum IconTheme { Scissors, WhaleGirl }
impl IconTheme {
    pub const ALL: [IconTheme; 2] = [...];        // 菜单顺序 = 注册顺序
    fn id() -> &'static str;                      // 持久化值 + 菜单项 ID 后缀
    fn label() -> &'static str;                   // 菜单展示文案
    fn is_template() -> bool;                     // 是否走 macOS 模板机制
    fn static_icon() -> Image<'static>;           // 静态图（关动画时用）
    fn frames() -> &'static [Image<'static>];     // 动画帧（各主题帧数不同）
    fn beats() -> &'static [usize];               // 播放顺序 = frames 的下标序列
}
```

`frames` 返回变长切片、`beats` 单独给播放顺序，是因为两套皮的动画不是一回事：
剪刀 4 帧往返开合（`[0,1,2,3,2,1]`，避免首尾同帧的顿感），鲸鱼娘 12 帧顺序播放（吃三口饭）。
菜单、勾选态、持久化全部由 `ALL` 驱动，**加皮肤不需要改 tray.rs**。

### 2.2 菜单与持久化

右键菜单新增子菜单「顶部图标 ▸」，每套皮肤一个勾选项：

```
打开主窗口
检查更新
──────────
CPU 动画
顶部图标 ▸  剪刀（默认） / DeepSeek 鲸鱼娘
──────────
退出 Prune
```

- 点击 → `switch_theme()`：写 `settings.json` 的 `trayIconTheme` → 立即重发静态图 → 搬勾选态
- 菜单项 ID 统一前缀 `tray-icon:`，事件分发走前缀匹配，`match` 里不用逐套加分支
- 启动时 `tray_icon::load()` 读取；文件缺失 / 缺键 / 值不认识 → 默认剪刀

`settings.json` 与 CPU 动画开关共用同一个文件，写入时只改本键（读-改-写，保留其它键）：

```json
{ "trayCpuAnim": true, "trayIconTheme": "whale" }
```

### 2.3 CPU → 吃饭速度（M11 核心）

动画循环每播一帧读一次整机 CPU 快照，换算成**这一帧的停留毫秒数**：

| CPU（10 s 均值） | 每帧 | 吃完一碗（12 帧） |
|---|---|---|
| 未知 | 700 ms | 8.4 s |
| < 5% | 800 ms | 9.6 s |
| 5–15% | 690 ms | 8.3 s |
| 15–30% | 520 ms | 6.2 s |
| 30–45% | 380 ms | 4.6 s |
| 45–60% | 280 ms | 3.4 s |
| 60–75% | 200 ms | 2.4 s |
| 75–88% | 140 ms | 1.7 s |
| ≥ 88% | 95 ms | 1.1 s |

- **没有静止档**：旧方案 CPU <10% 就回静态图，那等于「一张不动的头像看不出负载」。
  现在最闲也只是吃得慢，图标永远在动（CPU 未知同理，绝不拿未知值停掉表意）
- 快慢差 9 倍，隔着一屏也能分出「闲」和「忙」；精度由 tooltip 的 `Prune · CPU 12%` 负责
- 逐帧读 CPU 而不是逐轮读：快照本来 10 s 才刷新，逐帧读与逐轮读等价，
  但换皮肤/开关动画能立刻生效，不用等一轮跑完
- 换皮肤时 `LAST_FRAME` 预置成 `FRAME_DIRTY` 哨兵，绕过「相同帧不重发」的快路径，
  否则图标会停在旧皮肤上；`set_frame` 对越界索引静默跳过（beats 还指向上一个主题时会出现）

## 3. 素材

同人头像只有一张 1024×1024 立绘，直接缩到 48px 会糊成一团，所以裁到「头顶 → 下巴」并做圆形遮罩，
下半部用矢量画饭碗、白饭和筷子：

| 产出 | 尺寸 | 说明 |
|---|---|---|
| `icons/tray-whale/macos.png` / `win.png` | 48 / 32 | 静态图（= 第 0 帧，满碗） |
| `icons/tray-anim/whale/{macos,win}/f0..11.png` | 48 / 32 | 12 帧，一轮 = 吃三口饭 |

生成：`node scripts/gen-tray-whale.mjs`（确定性渲染，重跑 diff 应为空）。
源头：`assets/tray-whale.png`（1024×1024 同人头像，裁切框 `345,250,420,420`）。

**12 帧的叙事**：每 4 帧吃一口 —— 舀（尖端在碗里）→ 抬（离开饭面）→ 送（到右嘴角外）→ 咽（碗里饭少一截）。
饭碗有碗沿、碗身、碗足三段；白饭是碗沿上方凸起的白弧，宽度和高度随剩余饭量收缩，
第 11 帧见底后下一轮添满。筷子只到右嘴角外侧，不穿过眼睛/嘴 —— 48px 下穿脸会糊成一团黑。

矢量部分在 4 倍画布上渲染再缩回目标尺寸（超采样），避免 48px 直接画时的锯齿。

## 4. 新增一套皮肤

1. 出图：仿 `gen-tray-whale.mjs` 产出静态图 + N 帧（macOS 48px / Windows 32px，帧数自定）
2. `IconTheme` 加变体，补 `id` / `label` / `is_template` / `static_icon` / `frames` / `beats`
3. 加进 `ALL` 数组
4. `cargo test -p prune-app --lib`：
   - `every_theme_has_decodable_square_icons` 校验资源能解码、是正方形、帧数 ≥2
   - `every_beat_points_at_a_real_frame` 抓 `beats` 索引越界（改帧数最容易漏改这一处）

彩色皮肤记得 `is_template()` 返回 `false`，否则等于白做。

## 5. 验收

- [ ] 右键「顶部图标」能在剪刀 / 鲸鱼娘之间切换，图标立即变，无需重启
- [ ] 勾选态跟随当前主题，重启后保持
- [ ] CPU 动画开着时：空闲（<5%）吃得明显慢，满跑编译时一轮约 1 秒，肉眼能分出快慢
- [ ] 关掉「CPU 动画」→ 回各自静态图；再打开 → 最多 0.5 s 内恢复吃
- [ ] 深色 / 浅色菜单栏下鲸鱼娘都是彩色（没被反色成剪影）
- [ ] 剪掉 settings.json 或写成非法值 → 回落剪刀，不崩

## 6. 已知限制

- 鲸鱼娘是网友二创（同人）作品，仅适合个人使用；商用需授权。要干净的商用素材应重画或改用自绘形象
- 48px 里塞「头像 + 饭碗 + 筷子」，饭量递减这个维度在真实菜单栏尺寸下几乎看不出，
  主要靠筷子节奏和头部微动表达「在吃」；想强化饭量就得牺牲头像尺寸
- 只在托盘菜单里切换，面板内暂无切换 UI
- Windows 侧资源已生成但未实机验证（任务栏彩色图标逻辑与 W3 一致）
