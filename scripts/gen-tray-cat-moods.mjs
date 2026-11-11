// 生成「小猫 · 心情帧集」（M20，docs/f1-pet-mood-plan.md §3.2）
//
//   src-tauri/icons/tray-anim/cat-mood/pant/{macos,win}/f0..3.png  趴下喘气
//   src-tauri/icons/tray-anim/cat-mood/arch/{macos,win}/f0..3.png  弓背炸毛
//
// 与 gen-tray-cat.mjs 同族（几何常数 / 建模坐标系 / 渲染纪律全部沿用）：
//   - 单色实心剪影，macOS 黑色模板帧（系统反色）/ Windows #34D399
//   - 圆润优先：贝塞尔圆耳、椭圆吻部、颈椭圆填凹口、脚掌圆椭圆
//   - 眼睛 mask 镂空（模板图只认 alpha）
//   - 32×24 网格建模，输出 64×48（macos）/ 44×33（win）
//
// 姿态设计（18pt 下可读性靠轮廓剪影，不靠细节表情）：
//   pant 趴下喘气：身体压平贴地（躯干 ry 收窄、下移到地线），四肢收拢成短桩，
//     抬头张望；腹式呼吸 = 躯干 ry 振荡 + 轻微起伏。f3 尾巴轻扫（活物感）。
//   arch 弓背炸毛：万圣节弓背——躯干上拱（rot −18°）、头压低、尾巴竖直微摆、
//     四腿撑地原地小幅起伏。炸毛的「毛」不画（18pt 会糊），靠轮廓读。
//
// 狂奔不在此生成：复用 gen-tray-cat.mjs 的 12 帧前跃，高速播放即连续扑跳。
//
// 用法：node scripts/gen-tray-cat-moods.mjs（可重跑，确定性渲染）
import { mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
import sharp from "sharp";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const VIEW_W = 32;
const VIEW_H = 24;

/** 一条腿 = 腿杆 + 脚掌（与 gen-tray-cat.mjs 同构） */
function leg(x1, y1, x2, y2, w, pawRx, pawRy, color) {
  return (
    `<line x1="${x1.toFixed(2)}" y1="${y1.toFixed(2)}" x2="${x2.toFixed(2)}" y2="${y2.toFixed(2)}"
      stroke-width="${w}" stroke-linecap="round"/>` +
    `<ellipse cx="${x2.toFixed(2)}" cy="${y2.toFixed(2)}" rx="${pawRx}" ry="${pawRy}"
      fill="${color}" stroke="none"/>`
  );
}

/** 眼睛镂空 + 耳/吻（头系部件，坐标由调用方给定） */
function headParts(headX, headY, color, earShift) {
  const s = earShift ?? { dx: 0, dy: 0, rot: 0 };
  // 耳：小三角耳带圆尖（炸毛态整体后压 rot）
  const earL = `M4.7 8.0 C4.9 6.4, 5.2 5.3, 5.7 5.3 C6.3 5.5, 6.9 6.4, 7.2 7.6 Z`;
  const earR = `M7.4 7.5 C7.8 6.1, 8.3 5.0, 8.8 5.1 C9.2 5.4, 9.4 6.6, 9.3 7.9 Z`;
  const t = (x, y) =>
    `${x + s.dx} ${y + s.dy}`;
  return `
    <circle cx="${headX - 0.6}" cy="${headY - 0.5}" r="0.75" fill="#000"/>
    <path d="M${t(4.7, 8.0)} C4.9 6.4, 5.2 5.3, 5.7 5.3 C6.3 5.5, 6.9 6.4, 7.2 7.6 Z"
      fill="${color}" stroke="none" transform="rotate(${s.rot} ${headX} ${headY})"/>
    <path d="M${t(7.4, 7.5)} C7.8 6.1, 8.3 5.0, 8.8 5.1 C9.2 5.4, 9.4 6.6, 9.3 7.9 Z"
      fill="${color}" stroke="none" transform="rotate(${s.rot} ${headX} ${headY})"/>
    <ellipse cx="${headX - 2.1}" cy="${headY + 1.0}" rx="1.8" ry="1.3"
      transform="rotate(-6 ${headX - 2.1} ${headY + 1.0})" fill="${color}" stroke="none"/>`;
}

/** 耳/吻/眼洞的偏移：原始耳形按站立态头部 (7.0, 9.0) 设计，
 *  任何姿态都要按「当前头心 − 站立头心」平移，否则耳朵悬浮脱节 */
function earOffset(headX, headY) {
  return { dx: headX - 7.0, dy: headY - 9.0 };
}

// ---- pant：趴下喘气（4 帧呼吸循环）----
// breathe: 0 = 呼出（躯干最扁）→ 1 = 吸入（躯干鼓起 + 轻微上浮）
const PANT_BASE = {
  body: { x: 14.8, y: 15.6, rx: 6.6, rot: 0 },
  head: { x: 6.8, y: 12.0, r: 2.7 },
  neck: { x: 9.4, y: 13.6, rx: 2.6, ry: 2.4, rot: 24 },
  // 四肢收拢成短桩：腿根贴身体下缘，脚掌落地线（f0 同一条地线）
  hips: { front: { x: 11.2, y: 16.6 }, hind: { x: 18.6, y: 16.6 } },
  paws: { fN: [8.8, 19.7], fF: [10.3, 19.1], hN: [19.7, 19.7], hF: [21.1, 19.1] },
};

const PANT_POSES = [
  { breathe: 0, dx: 0, dy: 0, tailRot: 0 },
  { breathe: 1, dx: 0, dy: -0.28, tailRot: -3 },
  { breathe: 0, dx: 0, dy: 0, tailRot: 0 },
  { breathe: 1, dx: 0, dy: -0.28, tailRot: 9 },
];

/** 躯干 ry = 2.9（呼出）→ 3.6（吸入）；整猫随呼吸上浮 */
function pantFrameSvg(w, h, color, pose) {
  const b = pose.breathe;
  const bodyRy = 2.9 + 0.7 * b;
  const bodyY = PANT_BASE.body.y + (0.35 - 0.35 * b);
  const headY = PANT_BASE.head.y - 0.25 * b;
  const neckY = PANT_BASE.neck.y - 0.25 * b;
  const { body, head, neck, hips, paws } = PANT_BASE;

  // 尾巴：贴地平卷（比站立态整体下移），f3 轻扫
  const tail =
    `M20.8 15.2 ` +
    `C22.6 15.0, 23.6 13.8, 23.5 12.2 ` +
    `C23.4 10.6, 21.9 9.6, 20.4 10.2 ` +
    `C19.3 10.7, 18.8 11.9, 19.3 13.0`;

  return Buffer.from(
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${VIEW_W} ${VIEW_H}"
      fill="none" stroke="${color}" color="${color}"
      stroke-linecap="round" stroke-linejoin="round">
      <defs>
        <mask id="eye">
          <rect x="0" y="0" width="${VIEW_W}" height="${VIEW_H}" fill="#fff"/>
          <circle cx="${(head.x - 0.6).toFixed(2)}" cy="${(headY - 0.5).toFixed(2)}" r="0.75" fill="#000"/>
        </mask>
      </defs>
      <g transform="translate(${pose.dx} ${pose.dy})">
        <g transform="rotate(${pose.tailRot} 20.8 15.2)">
          <path d="${tail}" stroke-width="1.6"/>
        </g>
        <g mask="url(#eye)">
          <ellipse cx="${body.x}" cy="${bodyY}" rx="${body.rx}" ry="${bodyRy}"
            fill="${color}" stroke="none"/>
          <ellipse cx="${neck.x}" cy="${neckY}" rx="${neck.rx}" ry="${neck.ry}"
            transform="rotate(${neck.rot} ${neck.x} ${neckY})" fill="${color}" stroke="none"/>
          <circle cx="${head.x}" cy="${headY}" r="${head.r}" fill="${color}" stroke="none"/>
          ${(() => {
            const o = earOffset(head.x, headY);
            return `<path d="M4.7 8.0 C4.9 6.4, 5.2 5.3, 5.7 5.3 C6.3 5.5, 6.9 6.4, 7.2 7.6 Z"
              fill="${color}" stroke="none" transform="translate(${o.dx.toFixed(2)} ${o.dy.toFixed(2)})"/>
            <path d="M7.4 7.5 C7.8 6.1, 8.3 5.0, 8.8 5.1 C9.2 5.4, 9.4 6.6, 9.3 7.9 Z"
              fill="${color}" stroke="none" transform="translate(${o.dx.toFixed(2)} ${o.dy.toFixed(2)})"/>`;
          })()}
          <ellipse cx="${head.x - 2.1}" cy="${headY + 1.0}" rx="1.8" ry="1.3"
            transform="rotate(-6 ${head.x - 2.1} ${headY + 1.0})" fill="${color}" stroke="none"/>
        </g>
        <!-- 远侧腿（细、脚掌小） -->
        ${leg(hips.front.x, hips.front.y, paws.fF[0], paws.fF[1], 1.75, 1.08, 0.80, color)}
        ${leg(hips.hind.x, hips.hind.y, paws.hF[0], paws.hF[1], 1.75, 1.08, 0.80, color)}
        <!-- 近侧腿（粗、脚掌大） -->
        ${leg(hips.front.x, hips.front.y, paws.fN[0], paws.fN[1], 2.0, 1.32, 0.96, color)}
        ${leg(hips.hind.x, hips.hind.y, paws.hN[0], paws.hN[1], 2.0, 1.32, 0.96, color)}
      </g>
    </svg>`,
  );
}

// ---- arch：弓背炸毛（4 帧原地起伏 + 尾巴摇摆）----
const ARCH_BASE = {
  // 躯干上拱：上移 + rot −18°（背线朝上弓起）
  body: { x: 14.8, y: 10.6, rx: 5.8, ry: 4.0, rot: -18 },
  // 头压低在躯干前下方（典型警戒姿态：盯着威胁）
  head: { x: 8.4, y: 14.8, r: 2.6 },
  neck: { x: 10.4, y: 12.9, rx: 2.5, ry: 2.3, rot: 52 },
  hips: { front: { x: 11.4, y: 13.6 }, hind: { x: 18.4, y: 13.4 } },
  paws: { fN: [9.0, 19.7], fF: [10.5, 19.1], hN: [19.4, 19.7], hF: [20.9, 19.1] },
  // 尾巴竖直（炸毛警戒的经典竖尾），根部在臀后
  tailRoot: { x: 20.6, y: 10.6 },
};

const ARCH_POSES = [
  { dy: -0.2, tailRot: -7, headDy: 0 },
  { dy: 0, tailRot: 0, headDy: 0.1 },
  { dy: 0.2, tailRot: 7, headDy: 0.15 },
  { dy: 0, tailRot: 0, headDy: 0.1 },
];

function archFrameSvg(w, h, color, pose) {
  const { body, head, neck, hips, paws, tailRoot } = ARCH_BASE;
  const bodyY = body.y + pose.dy;
  const headY = head.y + pose.headDy;
  const neckY = neck.y + pose.dy + pose.headDy * 0.5;
  // 尾巴竖直上指（bristle tail），随 pose 摇摆
  const tail =
    `M${tailRoot.x} ${tailRoot.y} ` +
    `C21.6 8.2, 22.0 5.4, 21.4 2.6`;

  // 耳朵后压（警戒态）：整体旋转 −14°
  const earRot = -14;

  return Buffer.from(
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${VIEW_W} ${VIEW_H}"
      fill="none" stroke="${color}" color="${color}"
      stroke-linecap="round" stroke-linejoin="round">
      <defs>
        <mask id="eyeA">
          <rect x="0" y="0" width="${VIEW_W}" height="${VIEW_H}" fill="#fff"/>
          <circle cx="${(head.x - 0.6).toFixed(2)}" cy="${(headY - 0.5).toFixed(2)}" r="0.75" fill="#000"/>
        </mask>
      </defs>
      <g transform="translate(0 ${pose.dy})">
        <g transform="rotate(${pose.tailRot} ${tailRoot.x} ${tailRoot.y})">
          <path d="${tail}" stroke-width="1.7"/>
        </g>
        <g mask="url(#eyeA)">
          <ellipse cx="${body.x}" cy="${bodyY}" rx="${body.rx}" ry="${body.ry}"
            transform="rotate(${body.rot} ${body.x} ${bodyY})" fill="${color}" stroke="none"/>
          <ellipse cx="${neck.x}" cy="${neckY}" rx="${neck.rx}" ry="${neck.ry}"
            transform="rotate(${neck.rot} ${neck.x} ${neckY})" fill="${color}" stroke="none"/>
          <circle cx="${head.x}" cy="${headY}" r="${head.r}" fill="${color}" stroke="none"/>
          ${(() => {
            const o = earOffset(head.x, headY);
            return `<path d="M4.7 8.0 C4.9 6.4, 5.2 5.3, 5.7 5.3 C6.3 5.5, 6.9 6.4, 7.2 7.6 Z"
              fill="${color}" stroke="none"
              transform="translate(${o.dx.toFixed(2)} ${o.dy.toFixed(2)}) rotate(${earRot} ${head.x} ${headY})"/>
            <path d="M7.4 7.5 C7.8 6.1, 8.3 5.0, 8.8 5.1 C9.2 5.4, 9.4 6.6, 9.3 7.9 Z"
              fill="${color}" stroke="none"
              transform="translate(${o.dx.toFixed(2)} ${o.dy.toFixed(2)}) rotate(${earRot} ${head.x} ${headY})"/>`;
          })()}
          <ellipse cx="${head.x - 2.1}" cy="${headY + 1.0}" rx="1.8" ry="1.3"
            transform="rotate(-6 ${head.x - 2.1} ${headY + 1.0})" fill="${color}" stroke="none"/>
        </g>
        ${leg(hips.front.x, hips.front.y, paws.fF[0], paws.fF[1], 1.75, 1.08, 0.80, color)}
        ${leg(hips.hind.x, hips.hind.y, paws.hF[0], paws.hF[1], 1.75, 1.08, 0.80, color)}
        ${leg(hips.front.x, hips.front.y, paws.fN[0], paws.fN[1], 2.0, 1.32, 0.96, color)}
        ${leg(hips.hind.x, hips.hind.y, paws.hN[0], paws.hN[1], 2.0, 1.32, 0.96, color)}
      </g>
    </svg>`,
  );
}

// ---- 输出 ----
const SETS = [
  { dir: "macos", color: "#000000", w: 64, h: 48 },
  { dir: "win", color: "#34D399", w: 44, h: 33 },
];

for (const set of SETS) {
  for (const [mood, poses, render] of [
    ["pant", PANT_POSES, pantFrameSvg],
    ["arch", ARCH_POSES, archFrameSvg],
  ]) {
    const dir = path.join(root, "src-tauri/icons/tray-anim/cat-mood", mood, set.dir);
    await mkdir(dir, { recursive: true });
    for (const [i, pose] of poses.entries()) {
      await writeFile(
        path.join(dir, `f${i}.png`),
        await sharp(render(set.w, set.h, set.color, pose)).png().toBuffer(),
      );
    }
    console.log(`  cat-mood/${mood}/${set.dir}/f0..${poses.length - 1}.png (${set.w}×${set.h} ${set.color})`);
  }
}

console.log(`小猫心情帧集已生成（pant ${PANT_POSES.length} 帧 + arch ${ARCH_POSES.length} 帧）。`);
