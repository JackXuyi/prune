// 生成「蓝鲸 · 游泳看 CPU」托盘图标（M13 引入；M17 改为剪刀同款线稿风格）
//
//   src-tauri/icons/tray-whale-logo/{macos,win}.png              静态图（= 第 0 帧）
//   src-tauri/icons/tray-anim/whale-logo/{macos,win}/f0..7.png   8 帧游泳循环
//
// 注意：目录名 tray-whale-logo / whale-logo 是历史遗留（曾是 DeepSeek Logo 皮），
// Rust 侧 include_bytes! 引用这些路径，改目录要同步改 tray_icon.rs，这里不动。
//
// 风格与剪刀（gen-tray-frames.mjs）完全一致：
//   - Lucide 24 网格线稿，fill=none + 圆头描边，无徽章、无色块
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
// 8 帧 = 一次完整尾鳍往复：0 → 下压(+16°) → 回中 → 上扬(−16°) → 回中
const SEQ = [
  { a: 0, dy: 0 },
  { a: 9, dy: -0.35 },
  { a: 16, dy: -0.55 },
  { a: 9, dy: -0.35 },
  { a: 0, dy: 0 },
  { a: -9, dy: 0.35 },
  { a: -16, dy: 0.55 },
  { a: -9, dy: 0.35 },
];

// 鲸身：头在左（圆润大弧），向右收窄到尾根 (15.9, 12.6)。闭合轮廓，纯描边
const BODY =
  "M3 12.7 C3 9.8, 5.8 8.2, 9.2 8.6 " +
  "C12.3 9, 14.9 10.7, 15.9 12.6 " +
  "C15.2 14.8, 12.6 16.6, 9.4 16.6 " +
  "C5.9 16.6, 3 15.4, 3 12.7 Z";

// 尾鳍：上下两瓣各一条开放曲线，枢轴在尾根。旋转角由 SEQ[a] 给出
const TAIL_UP = "M15.9 12.6 C17.8 11.8, 18.9 10, 19.1 7.9";
const TAIL_DOWN = "M15.9 12.6 C17.8 13.4, 18.9 15.2, 19.1 17.3";
const PIVOT = "15.9 12.6";

// 胸鳍：身体内的一小笔弧线，增加鲸的可辨识度
const FIN = "M9.5 13.2 C10.3 14.2, 11.5 14.6, 12.6 14.4";

// 头顶小喷水：两道短弧（克制装饰）
const SPOUT =
  "M6.5 8 C6.3 7, 5.7 6.3, 4.9 5.9 " +
  "M6.5 8 C6.7 7, 7.3 6.3, 8.1 5.9";

// 眼睛：实心小点（Lucide 亦用实点表达，点不参与描边）
const EYE = '<circle cx="6.5" cy="12" r="0.9" fill="COLOR" stroke="none"/>';

function frameSvg(size, color, strokeWidth, { a, dy }) {
  return Buffer.from(
    `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 24 24"
      fill="none" stroke="${color}" stroke-width="${strokeWidth}"
      stroke-linecap="round" stroke-linejoin="round">
      <g transform="translate(0 ${dy.toFixed(2)})">
        <!-- 尾鳍（先画，被身体压住衔接处） -->
        <g transform="rotate(${a.toFixed(2)} ${PIVOT})">
          <path d="${TAIL_UP}"/>
          <path d="${TAIL_DOWN}"/>
        </g>
        <path d="${BODY}"/>
        <path d="${FIN}"/>
        <path d="${SPOUT}"/>
        ${EYE.replaceAll("COLOR", color)}
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
