// 生成「蓝鲸 · 游泳看 CPU」托盘图标（M13 引入；M17 改为剪刀同款线稿风格）
//
//   src-tauri/icons/tray-whale-logo/{macos,win}.png              静态图（= 第 0 帧）
//   src-tauri/icons/tray-anim/whale-logo/{macos,win}/f0..7.png   8 帧游泳循环
//
// 注意：目录名 tray-whale-logo / whale-logo 是历史遗留（曾是 DeepSeek Logo 皮），
// Rust 侧 include_bytes! 引用这些路径，改目录要同步改 tray_icon.rs，这里不动。
//
// 风格与剪刀（gen-tray-frames.mjs）同族：
//   - 无徽章、单色字形；实心剪影。M19 起画布加宽为 4:3，鲸鱼拉长、
//     尾鳍力臂变长，摆动端点位移在菜单栏尺寸下肉眼可辨
//   - 动画三重协同（真实游泳姿态）：尾鳍 ±25° 往复 + 身体反向俯仰 ±7°
//     （尾下压 → 头抬起）+ 纵向起伏，只动尾巴在 ~34px 下几乎看不出来
//   - 眼睛用 evenodd 透明镂空（macOS 模板图只认 alpha，白点会被染成同色）
//   - macOS  64×48 黑色模板帧（系统按菜单栏深浅反色）
//   - Windows 44×33 品牌色帧 #34D399（深色任务栏下黑色不可见）
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

// 每帧：尾鳍角度（deg，绕尾根枢轴）+ 身体俯仰（deg，绕身体中心，与尾鳍反相）
// + 身体纵向偏移（32×24 网格 px，与尾鳍反相：尾下压 → 身体上浮）。
// 6 帧大步进（M20）：0 → 下压(+30°) → 回收 → 中位 → 上扬(−30°) → 回收，
// 首尾同相位无缝循环；每帧角增量最大 30°，低分辨率下跳变清晰可辨。
// 想回到顺滑优先就加回中间帧（±15° 各两档 → 8 帧，同步改 tray_icon.rs 的
// WHALE_LOGO_BEATS 与 include_bytes 数组）。
const SEQ = [
  { a: 0, dy: 0 },
  { a: 30, dy: -0.9 },
  { a: 15, dy: -0.45 },
  { a: 0, dy: 0 },
  { a: -30, dy: 0.9 },
  { a: -15, dy: 0.45 },
];
// 身体俯仰与尾鳍反相：尾下压时头抬起（负角 = SVG 逆时针 = 顺泳姿）
const PITCH_RATIO = -0.4;

// 鲸身（实心剪影，32×24 网格，比旧版拉长 1.33 倍给尾鳍留力臂）：
// 头在左，向右收窄到尾根 (21.6, 12.4)。
// 与 EYE_HOLE 拼成单一路径走 evenodd——眼睛是透明镂空而非白点，
// macOS 模板图只取 alpha 通道，白点会被染成与身体同色而消失。
const BODY =
  "M1.5 12.6 C1.5 8.6, 5.4 6.2, 10.4 6.7 " +
  "C15.6 7.3, 19.9 9.6, 21.6 12.4 " +
  "C20.7 15.5, 16.7 18, 11.6 18 " +
  "C5.8 18, 1.5 16.3, 1.5 12.6 Z";
const EYE_HOLE =
  "M6.9 11.4 A1.15 1.15 0 1 0 4.6 11.4 A1.15 1.15 0 1 0 6.9 11.4 Z";
const BODY_PIVOT = "12 12";

// 尾鳍（实心双叶，叶尖 (25.3, 6.0) / (25.3, 19.2)），枢轴在尾根。
// ±25° 摆动时叶尖纵向行程约 3.5 网格单位（48px 高下约 7px），肉眼可辨
const TAIL =
  "M20.9 12.3 C23.4 11.4, 24.9 9.2, 25.3 6.0 " +
  "C27.2 7.9, 27.9 10.6, 27.0 12.6 " +
  "C27.9 14.6, 27.2 17.3, 25.3 19.2 " +
  "C24.9 15.9, 23.4 13.6, 20.9 12.6 Z";
const TAIL_PIVOT = "21.2 12.45";

// 头顶小喷水：两道短弧（克制装饰，俯仰时随身体一起动）
const SPOUT =
  "M7 6 C6.7 4.8, 6 4, 5 3.4 " +
  "M7 6 C7.3 4.8, 8 4, 9 3.4";

function frameSvg(w, h, color, strokeWidth, { a, dy }) {
  const pitch = (a * PITCH_RATIO).toFixed(2);
  return Buffer.from(
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 32 24"
      fill="none" stroke="${color}" stroke-width="${strokeWidth}"
      stroke-linecap="round" stroke-linejoin="round">
      <!-- 身体俯仰 + 起伏包住整只鲸，尾鳍在此之上再做相对摆动 -->
      <g transform="translate(0 ${dy.toFixed(2)}) rotate(${pitch} ${BODY_PIVOT})">
        <!-- 尾鳍（先画，被身体压住衔接处，同为实心无痕拼接） -->
        <g transform="rotate(${a.toFixed(2)} ${TAIL_PIVOT})">
          <path d="${TAIL}" fill="${color}" stroke="none"/>
        </g>
        <path d="${BODY} ${EYE_HOLE}" fill="${color}" stroke="none" fill-rule="evenodd"/>
        <path d="${SPOUT}"/>
      </g>
    </svg>`,
  );
}

const SETS = [
  { dir: "macos", color: "#000000", w: 64, h: 48, strokeWidth: 1.9 },
  { dir: "win", color: "#34D399", w: 44, h: 33, strokeWidth: 2 },
];

for (const set of SETS) {
  const staticPath = path.join(root, "src-tauri/icons/tray-whale-logo", `${set.dir}.png`);
  await mkdir(path.dirname(staticPath), { recursive: true });
  await writeFile(staticPath, await sharp(frameSvg(set.w, set.h, set.color, set.strokeWidth, SEQ[0])).png().toBuffer());

  const animDir = path.join(root, "src-tauri/icons/tray-anim/whale-logo", set.dir);
  await mkdir(animDir, { recursive: true });
  for (const [i, frame] of SEQ.entries()) {
    await writeFile(
      path.join(animDir, `f${i}.png`),
      await sharp(frameSvg(set.w, set.h, set.color, set.strokeWidth, frame)).png().toBuffer(),
    );
  }
  console.log(`  tray-whale-logo/${set.dir}.png + tray-anim/whale-logo/${set.dir}/f0..${SEQ.length - 1}.png (${set.w}×${set.h} ${set.color})`);
}

console.log(`蓝鲸线稿「游泳看 CPU」图标已生成（${SEQ.length} 帧，剪刀同款风格）。`);
