// 生成托盘动画帧（M6，docs/m6-tray-anim-plan.md §3.2 / §4.2）：
//   4 帧 = 剪刀开合 闭合(f0) → 半开(f1) → 全开(f2) → 半开(f3，=f1 镜像节拍)
//   动画序列 f0→f1→f2→f3→f2→f1（6 拍往返，避免「跳变回闭合」的生硬感）
//
// 造型来源 assets/scissors.svg（Lucide 24 网格，与 gen-icons.mjs 同源）：
//   - 刀刃是两条过枢轴 (12,12) 的对角线（基准位 ±45°，即图标默认的 90° 开角）
//   - 手柄是两个圆 (6,6)/(6,18)，**固定不动** —— 只转刀刃角度，
//     避免换帧时图标重心晃动（方案 §6 风险项）
//
// 产出两套（方案 §3.2）：
//   macOS  48×48 黑色模板帧 → src-tauri/icons/tray-anim/macos/f{0..3}.png
//   Windows 32×32 品牌色帧 → src-tauri/icons/tray-anim/win/f{0..3}.png
//   （W3 教训：深色任务栏下黑色模板图不可见，Windows 必须给品牌色）
//
// 用法：node scripts/gen-tray-frames.mjs（可重跑，确定性渲染，重跑 git diff 应为空）
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import sharp from "sharp";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
import { fileURLToPath } from "node:url";

// Lucide scissors 的刀刃线段（assets/scissors.svg），枢轴 (12,12)。
// 刀刃 A = 上右尖（长线）；刀刃 B = 下右尖（两段共线，圆环遮挡处断开）。
const BLADE_A = ["M20 4 8.12 15.88"];
const BLADE_B = ["M8.12 8.12 12 12", "M14.8 14.8 20 20"];
const HANDLES = ['<circle cx="6" cy="6" r="3"/>', '<circle cx="6" cy="18" r="3"/>'];
const PIVOT = "12 12";

// 各帧的附加旋转角（°，正值顺时针）。基准位 90° 开角：
//   f0 闭合：两刃各向中间转 43°（留 4° 缝隙保持两刃可辨）
//   f1 半开：转一半
//   f2 全开：0（即图标默认姿态）
//   f3 = f1 的镜像节拍（同角度，独立文件，维持「4 帧」资产契约）
const FRAMES = [
  { name: "f0", a: 43, b: -43 },
  { name: "f1", a: 21.5, b: -21.5 },
  { name: "f2", a: 0, b: 0 },
  { name: "f3", a: 21.5, b: -21.5 },
];

const strokeAttrs = 'fill="none" stroke-linecap="round" stroke-linejoin="round"';
const rot = (deg) => `transform="rotate(${deg} ${PIVOT})"`;

function frameSvg(frame, color, size, strokeWidth) {
  const blades = [
    ...BLADE_A.map((d) => `<path d="${d}" ${rot(frame.a)}/>`),
    ...BLADE_B.map((d) => `<path d="${d}" ${rot(frame.b)}/>`),
  ].join("");
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 24 24" ${strokeAttrs} stroke="${color}" stroke-width="${strokeWidth}">${blades}${HANDLES.join("")}</svg>`;
}

const SETS = [
  { dir: "macos", color: "#000000", size: 48, strokeWidth: 1.9 },
  { dir: "win", color: "#34D399", size: 32, strokeWidth: 2 },
];

for (const set of SETS) {
  const outDir = path.join(root, "src-tauri/icons/tray-anim", set.dir);
  await mkdir(outDir, { recursive: true });
  for (const frame of FRAMES) {
    const png = await sharp(Buffer.from(frameSvg(frame, set.color, set.size, set.strokeWidth)))
      .png()
      .toBuffer();
    await writeFile(path.join(outDir, `${frame.name}.png`), png);
  }
  console.log(`  src-tauri/icons/tray-anim/${set.dir}/f{0..3}.png  (${set.size}×${set.size} ${set.color})`);
}
console.log("托盘动画帧已生成。");
