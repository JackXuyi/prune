// 生成「蓝鲸 · 游泳看 CPU」托盘图标（M13 引入；M17 改为剪刀同款线稿风格）
//
//   src-tauri/icons/tray-whale-logo/{macos,win}.png              静态图（= 第 0 帧）
//   src-tauri/icons/tray-anim/whale-logo/{macos,win}/f0..7.png   8 帧游泳循环
//
// 注意：目录名 tray-whale-logo / whale-logo 是历史遗留（曾是 DeepSeek Logo 皮），
// Rust 侧 include_bytes! 引用这些路径，改目录要同步改 tray_icon.rs，这里不动。
//
// 风格与剪刀（gen-tray-frames.mjs）同族：
//   - 24 网格、无徽章、单色字形；M18 起鲸鱼改**实心剪影**并放大到几乎占满画布，
//     尾鳍摆动在 48px 下更醒目（眼睛用 evenodd 透明镂空，保住模板 alpha 语义）
//   - macOS  48×48 黑色模板帧（系统按菜单栏深浅反色）
//   - Windows 32×32 品牌色帧 #34D399（深色任务栏下黑色不可见）
//   - macOS 侧走 is_template()，与剪刀共用同一套模板机制
//
// 为什么是 8 帧：托盘图标同时是 CPU 表。尾鳍摆动节奏由 CPU 占用决定
// （tray.rs 的 anim_beat_ms）——CPU 越忙游得越急，8 帧 = 一次完整尾鳍往复
// （下压 → 摆平 → 上扬 → 回位）。
//
// 用法：node scripts/gen-tray-whale.mjs（可重跑，确定性渲染，重跑 git diff 应为空）
import { mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
import sharp from "sharp";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

// 每帧：尾鳍角度（deg，绕尾根枢轴）+ 身体纵向偏移（24 网格 px）
// 8 帧 = 一次完整尾鳍往复：0 → 下压(+18°) → 回中 → 上扬(−18°) → 回中
// 实心剪影比线稿大一号，摆幅也从 ±16° 加到 ±18°，游动感更明显
const SEQ = [
  { a: 0, dy: 0 },
  { a: 10, dy: -0.45 },
  { a: 18, dy: -0.7 },
  { a: 10, dy: -0.45 },
  { a: 0, dy: 0 },
  { a: -10, dy: 0.45 },
  { a: -18, dy: 0.7 },
  { a: -10, dy: 0.45 },
];

// 鲸身（实心剪影，放大到几乎占满 24 网格）：头在左，向右收窄到尾根 (17.5, 12.4)。
// 与 EYE_HOLE 拼成单一路径走 evenodd——眼睛是透明镂空而非白点，
// macOS 模板图只取 alpha 通道，白点会被染成与身体同色而消失。
const BODY =
  "M1.5 12.6 C1.5 8.8, 5 6.4, 9.6 6.9 " +
  "C13.9 7.4, 16.5 9.6, 17.5 12.4 " +
  "C16.7 15.4, 13.2 17.9, 9.4 17.9 " +
  "C4.8 17.9, 1.5 16.2, 1.5 12.6 Z";
const EYE_HOLE =
  "M6.7 11.5 A1.1 1.1 0 1 0 4.5 11.5 A1.1 1.1 0 1 0 6.7 11.5 Z";

// 尾鳍（实心双叶，叶尖 (20.5, 6.9) / (20.5, 18.1)），枢轴在尾根。旋转角由 SEQ[a] 给出
const TAIL =
  "M16.9 12.3 C18.9 11.5, 20.2 9.6, 20.5 6.9 " +
  "C22 8.5, 22.6 10.8, 21.9 12.5 " +
  "C22.6 14.2, 22 16.5, 20.5 18.1 " +
  "C20.2 15.4, 18.9 13.4, 16.9 12.6 Z";
const PIVOT = "17 12.45";

// 头顶小喷水：两道短弧（线稿残留的克制装饰，实心剪影下仍可辨）
const SPOUT =
  "M5.8 6 C5.5 4.8, 4.8 4, 3.8 3.5 " +
  "M5.8 6 C6.1 4.8, 6.8 4, 7.8 3.5";

function frameSvg(size, color, strokeWidth, { a, dy }) {
  return Buffer.from(
    `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 24 24"
      fill="none" stroke="${color}" stroke-width="${strokeWidth}"
      stroke-linecap="round" stroke-linejoin="round">
      <g transform="translate(0 ${dy.toFixed(2)})">
        <!-- 尾鳍（先画，被身体压住衔接处，同为实心无痕拼接） -->
        <g transform="rotate(${a.toFixed(2)} ${PIVOT})">
          <path d="${TAIL}" fill="${color}" stroke="none"/>
        </g>
        <path d="${BODY} ${EYE_HOLE}" fill="${color}" stroke="none" fill-rule="evenodd"/>
        <path d="${SPOUT}"/>
      </g>
    </svg>`,
  );
}

const SETS = [
  { dir: "macos", color: "#000000", size: 48, strokeWidth: 1.9 },
  { dir: "win", color: "#34D399", size: 32, strokeWidth: 2 },
];

for (const set of SETS) {
  const staticPath = path.join(root, "src-tauri/icons/tray-whale-logo", `${set.dir}.png`);
  await mkdir(path.dirname(staticPath), { recursive: true });
  await writeFile(staticPath, await sharp(frameSvg(set.size, set.color, set.strokeWidth, SEQ[0])).png().toBuffer());

  const animDir = path.join(root, "src-tauri/icons/tray-anim/whale-logo", set.dir);
  await mkdir(animDir, { recursive: true });
  for (const [i, frame] of SEQ.entries()) {
    await writeFile(
      path.join(animDir, `f${i}.png`),
      await sharp(frameSvg(set.size, set.color, set.strokeWidth, frame)).png().toBuffer(),
    );
  }
  console.log(`  tray-whale-logo/${set.dir}.png + tray-anim/whale-logo/${set.dir}/f0..${SEQ.length - 1}.png (${set.size}×${set.size} ${set.color})`);
}

console.log(`蓝鲸线稿「游泳看 CPU」图标已生成（${SEQ.length} 帧，剪刀同款风格）。`);
