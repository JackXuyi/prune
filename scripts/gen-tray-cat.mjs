// 生成「小猫 · 跳跃看 CPU」托盘图标（M22 取代鲸鱼皮）
//
//   src-tauri/icons/tray-cat/{macos,win}.png              静态图（= 第 0 帧）
//   src-tauri/icons/tray-anim/cat/{macos,win}/f0..7.png   8 帧前跃循环
//
// 风格与剪刀（gen-tray-frames.mjs）同族：
//   - 单色实心剪影，无徽章；macOS 黑色模板帧（系统按菜单栏深浅反色），
//     Windows 品牌色 #34D399（深色任务栏下黑色不可见）
//   - 造型参考 Framework7 的 cat 图标（M22 用户指定）：细长身形、头位高、
//     S 形上钩尾、四条清晰的腿。躯干 ry 2.8（细长但不干瘦）、头 r 3.0
//   - **圆润优先**（M22 四改）：所有转折点都是曲线或圆头，没有尖角——
//     耳根用三次贝塞尔收成圆尖（三角形尖耳在 18pt 下会糊成锯齿），
//     吻部用椭圆凸起（尖三角会读成鸟喙），头与躯干之间加一个颈部椭圆
//     消除两圆相切的凹口
//   - 爪：每条腿 = 腿杆（细）+ 脚掌（圆椭圆，可随姿态俯仰）。只用腿杆会在
//     18pt 下读成四根细棍，脚掌是「猫站在/踩在地上」的重量来源
//   - 眼睛走 mask 镂空（macOS 模板图只认 alpha，白点会被染成同色而消失）
//
// 为什么 8 帧：托盘图标同时是 CPU 表。跳跃频率由 tray.rs 的 anim_beat_ms 决定
// （CPU 越忙跳得越急）。8 帧 = 一次完整前跃，起跳/腾空/落地各有独立过渡拍：
//   0 站立 → 1 蓄力（微蹲抬前脚）→ 2 前脚高抬（后腿压缩）
//   → 3 蹬地起跳 → 4 腾空最高（拉平前扑）→ 5 下落前爪前探
//   → 6 **前爪触地**（后爪仍高悬，重心压在前爪）→ 7 **后爪落地**缓冲 → 回 0
// 「前脚先抬」是整跳的启动信号；落地拆成 6/7 两拍且后爪在 f6 明显悬空，
// 才看得出「先前爪着地、身体压下来、后爪随后收拢」的完整顺序，
// 而不是「四爪同时着地」或「飞出去突然站住」。
//
// dx 是锯齿循环：f6 落点最靠前（-2.5），f7 已向回收（-0.8），f0 归零——
// 每轮净位移回零，循环无缝（M21 鲸鱼前冲同款手法）。
//
// 用法：node scripts/gen-tray-cat.mjs（可重跑，确定性渲染）
import { mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
import sharp from "sharp";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

// ---- 几何常量（32×24 网格，猫头朝左，向左前跃）----
const VIEW_W = 32;
const VIEW_H = 24;

const HEAD = { x: 6.9, y: 7.4, r: 3.0 };
// 颈部椭圆：填掉「头球」与「身椭」相切处的凹口，让轮廓连续圆润
const NECK = { x: 9.5, y: 9.3, rx: 2.7, ry: 2.4, rot: 22 };
const BODY = { x: 14.9, y: 10.5, rx: 6.8, ry: 2.8, rot: 9 }; // 细长躯干，胸高臀低
const TAIL_ROOT = { x: 20.6, y: 10.8 };
const HIP_FRONT = { x: 10.2, y: 12.0 };
const HIP_HIND = { x: 19.4, y: 12.6 };
const TAIL_W = 1.4;
const LEG_W_NEAR = 1.9;
const LEG_W_FAR = 1.65;

// 躯干俯仰枢轴（约在身体中段）
const PIVOT = { x: 13.0, y: 11.0 };

// ---- 8 帧前跃姿态（坐标均为网格绝对值）----
// pitch：躯干+头+尾的整体俯仰（deg，正 = 头下沉）；dx/dy：整只猫的位移
// （负 dy = 升空）；foot：脚掌俯仰（0 = 平贴地面，正 = 脚尖下垂）；
// fN/fF/hN/hF：前近/前远/后近/后远脚掌落点。
const POSES = [
  // f0 站立（= 静态图）：四脚落地，尾上钩——F7 猫的标志剪影
  { pitch: 0, dx: 0, dy: 0, tail: 0, foot: 0,
    fN: [9.6, 19.7], fF: [11.0, 19.1], hN: [19.9, 19.7], hF: [21.2, 19.1] },
  // f1 蓄力：微蹲，近前脚离地起势——「准备跳」的启动拍
  { pitch: -3, dx: 0.15, dy: 0.6, tail: 6, foot: 10,
    fN: [9.0, 18.2], fF: [11.0, 19.1], hN: [19.9, 19.7], hF: [21.2, 19.1] },
  // f2 前脚高抬：前脚收至胸前，后腿压缩到底，尾上扬配平
  { pitch: -7, dx: 0.1, dy: 0.8, tail: 10, foot: 26,
    fN: [8.2, 16.2], fF: [10.8, 18.9], hN: [20.2, 19.7], hF: [21.5, 19.2] },
  // f3 蹬地起跳：后腿蹬直，身体前倾拉长，前脚向前探
  { pitch: -8, dx: -0.9, dy: -0.9, tail: -4, foot: 30,
    fN: [7.4, 16.4], fF: [9.0, 16.9], hN: [22.2, 17.6], hF: [23.0, 17.2] },
  // f4 腾空最高：身体拉平前扑，前脚前伸、后腿后拖，尾拉平
  { pitch: -2, dx: -1.9, dy: -2.2, tail: -14, foot: 28,
    fN: [5.9, 15.6], fF: [7.5, 15.5], hN: [22.8, 15.9], hF: [23.6, 15.6] },
  // f5 下落：身体下沉，前爪向前下方探出，后爪仍收在腹下
  { pitch: 4, dx: -2.4, dy: -1.0, tail: -8, foot: 14,
    fN: [6.8, 18.0], fF: [8.3, 17.3], hN: [21.6, 15.8], hF: [22.4, 15.4] },
  // f6 前爪触地：前爪落地承重、身体前倾压过去，后爪明显悬空（离地约 4.5）
  { pitch: 8, dx: -2.5, dy: 0.0, tail: 2, foot: 2,
    fN: [7.4, 19.7], fF: [8.9, 19.1], hN: [21.0, 15.4], hF: [21.8, 15.0] },
  // f7 后爪落地 + 缓冲：后爪收拢着地、四爪归位，重心压低后回到 f0
  { pitch: 3, dx: -0.9, dy: 0.5, tail: 7, foot: 0,
    fN: [9.2, 19.7], fF: [10.5, 19.1], hN: [19.9, 19.7], hF: [21.2, 19.1] },
];

/** 躯干俯仰 + 整体位移后某点的新位置（腿根跟着身体走，脚掌另行给定） */
function bodyPoint(x, y, p) {
  const rad = (p.pitch * Math.PI) / 180;
  const cos = Math.cos(rad);
  const sin = Math.sin(rad);
  return {
    x: PIVOT.x + (x - PIVOT.x) * cos - (y - PIVOT.y) * sin + p.dx,
    y: PIVOT.y + (x - PIVOT.x) * sin + (y - PIVOT.y) * cos + p.dy,
  };
}

/** 一条腿 = 腿杆 + 脚掌。腿杆细、脚掌粗，18pt 下才有「爪」的重量感 */
function leg(x1, y1, x2, y2, w, pawRx, pawRy, footDeg, color) {
  return (
    `<line x1="${x1.toFixed(2)}" y1="${y1.toFixed(2)}" x2="${x2.toFixed(2)}" y2="${y2.toFixed(2)}"
      stroke-width="${w}" stroke-linecap="round"/>` +
    `<ellipse cx="${x2.toFixed(2)}" cy="${y2.toFixed(2)}" rx="${pawRx}" ry="${pawRy}"
      transform="rotate(${footDeg} ${x2.toFixed(2)} ${y2.toFixed(2)})" fill="${color}" stroke="none"/>`
  );
}

function frameSvg(w, h, color, f) {
  const hipF = bodyPoint(HIP_FRONT.x, HIP_FRONT.y, f);
  const hipH = bodyPoint(HIP_HIND.x, HIP_HIND.y, f);

  // 尾巴：从臀后先上扬再向左回勾（S 钩），F7 猫的标志剪影
  const tail =
    `M${TAIL_ROOT.x} ${TAIL_ROOT.y} ` +
    `C23.6 9.6, 24.9 7.4, 24.7 5.2 ` +
    `C24.6 3.7, 23.5 2.9, 22.3 3.2`;

  // 耳：圆角三角——尖角在 18pt 缩放下会糊成锯齿，这里用贝塞尔把顶端收圆
  const earL = `M4.5 6.3 C4.8 4.2, 5.2 2.8, 5.8 2.8 C6.6 3.0, 7.4 4.4, 7.9 6.1 Z`;
  const earR = `M7.8 5.5 C8.3 4.1, 8.9 2.9, 9.4 3.0 C9.9 3.3, 10.1 4.9, 10.0 6.7 Z`;

  return Buffer.from(
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${VIEW_W} ${VIEW_H}"
      fill="none" stroke="${color}" color="${color}"
      stroke-linecap="round" stroke-linejoin="round">
      <defs>
        <mask id="eye">
          <rect x="0" y="0" width="${VIEW_W}" height="${VIEW_H}" fill="#fff"/>
          <!-- 眼睛：透明镂空（模板图只认 alpha） -->
          <circle cx="6.3" cy="6.9" r="0.82" fill="#000"/>
        </mask>
      </defs>
      <!-- 前跃位移 + 躯干俯仰作用于头身尾；腿在外部单独画（脚掌落点逐帧给定） -->
      <g transform="translate(${f.dx.toFixed(2)} ${f.dy.toFixed(2)}) rotate(${f.pitch} ${PIVOT.x} ${PIVOT.y})">
        <!-- 尾巴（在身体之后，被躯干压住根部；随跳跃甩动） -->
        <g transform="rotate(${f.tail} ${TAIL_ROOT.x} ${TAIL_ROOT.y})">
          <path d="${tail}" stroke-width="${TAIL_W}"/>
        </g>
        <g mask="url(#eye)">
          <ellipse cx="${BODY.x}" cy="${BODY.y}" rx="${BODY.rx}" ry="${BODY.ry}"
            transform="rotate(${BODY.rot} ${BODY.x} ${BODY.y})" fill="${color}" stroke="none"/>
          <!-- 颈部：把「球头」和「椭圆身」之间那道凹口填平 -->
          <ellipse cx="${NECK.x}" cy="${NECK.y}" rx="${NECK.rx}" ry="${NECK.ry}"
            transform="rotate(${NECK.rot} ${NECK.x} ${NECK.y})" fill="${color}" stroke="none"/>
          <circle cx="${HEAD.x}" cy="${HEAD.y}" r="${HEAD.r}" fill="${color}" stroke="none"/>
          <path d="${earL}" fill="${color}" stroke="none"/>
          <path d="${earR}" fill="${color}" stroke="none"/>
          <!-- 吻部：椭圆凸起（圆润，且不会读成鸟喙） -->
          <ellipse cx="4.9" cy="8.9" rx="2.05" ry="1.45"
            transform="rotate(-8 4.9 8.9)" fill="${color}" stroke="none"/>
        </g>
      </g>
      <!-- 远侧两条腿先画（较细、脚掌略小），形成前后层次 -->
      ${leg(hipF.x, hipF.y, f.fF[0], f.fF[1], LEG_W_FAR, 1.08, 0.80, f.foot, color)}
      ${leg(hipH.x, hipH.y, f.hF[0], f.hF[1], LEG_W_FAR, 1.08, 0.80, f.foot, color)}
      <!-- 近侧两条腿（粗、脚掌大而圆） -->
      ${leg(hipF.x, hipF.y, f.fN[0], f.fN[1], LEG_W_NEAR, 1.32, 0.96, f.foot, color)}
      ${leg(hipH.x, hipH.y, f.hN[0], f.hN[1], LEG_W_NEAR, 1.32, 0.96, f.foot, color)}
    </svg>`,
  );
}

const SETS = [
  { dir: "macos", color: "#000000", w: 64, h: 48 },
  { dir: "win", color: "#34D399", w: 44, h: 33 },
];

for (const set of SETS) {
  const staticPath = path.join(root, "src-tauri/icons/tray-cat", `${set.dir}.png`);
  await mkdir(path.dirname(staticPath), { recursive: true });
  await writeFile(staticPath, await sharp(frameSvg(set.w, set.h, set.color, POSES[0])).png().toBuffer());

  const animDir = path.join(root, "src-tauri/icons/tray-anim/cat", set.dir);
  await mkdir(animDir, { recursive: true });
  for (const [i, frame] of POSES.entries()) {
    await writeFile(
      path.join(animDir, `f${i}.png`),
      await sharp(frameSvg(set.w, set.h, set.color, frame)).png().toBuffer(),
    );
  }
  console.log(`  tray-cat/${set.dir}.png + tray-anim/cat/${set.dir}/f0..${POSES.length - 1}.png (${set.w}×${set.h} ${set.color})`);
}

console.log(`小猫「跳跃看 CPU」图标已生成（${POSES.length} 帧）。`);
