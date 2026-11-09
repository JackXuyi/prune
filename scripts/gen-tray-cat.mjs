// 生成「小猫 · 跳跃看 CPU」托盘图标（M22 取代鲸鱼皮）
//
//   src-tauri/icons/tray-cat/{macos,win}.png              静态图（= 第 0 帧）
//   src-tauri/icons/tray-anim/cat/{macos,win}/f0..11.png  12 帧前跃循环
//
// 风格与剪刀（gen-tray-frames.mjs）同族：
//   - 单色实心剪影，无徽章；macOS 黑色模板帧（系统按菜单栏深浅反色），
//     Windows 品牌色 #34D399（深色任务栏下黑色不可见）
//   - 造型参考用户提供的黑猫剪影：敦实身形、背部微拱、短粗腿、小三角耳、
//     尾巴上扬卷成开口朝内的圆钩。**头朝左、向左前跃**（与 M22 前几版一致）
//   - **圆润优先**：所有转折点都是曲线或圆头，没有尖角——耳根用三次贝塞尔
//     收成圆尖（三角尖耳在 18pt 下会糊成锯齿），吻部用椭圆凸起（尖三角会
//     读成鸟喙），头与躯干之间加一个颈部椭圆消除两圆相切的凹口
//   - 爪：每条腿 = 腿杆（粗）+ 脚掌（圆椭圆，可随姿态俯仰）。只用腿杆会在
//     18pt 下读成四根细棍，脚掌是「猫站在/踩在地上」的重量来源
//   - 眼睛走 mask 镂空（macOS 模板图只认 alpha，白点会被染成同色而消失）
//
// 为什么 12 帧（M22 六改）：托盘图标同时是 CPU 表，跳跃频率由 tray.rs 的
// anim_beat_ms 决定（CPU 越忙跳得越急）。8 帧时每个动作阶段只有 1 拍，
// 腾空与落地都「一步到位」，看起来像被弹弓弹出去。12 帧把弹道拆细：
//   0 站立 → 1-2 蓄力下蹲（前脚离地）→ 3 蹬地 → 4-5 上升
//   → 6 腾空顶点（四腿收拢）→ 7 下落 → 8 前爪触地（后爪高悬）
//   → 9 重心前移 → 10 后爪落地 → 11 起身回位
// 竖直方向是一条抛物弹道（顶点附近慢、两端快），所以定速播放下也像有重力。
//
// dx 是锯齿循环：f8 落点最靠前（-2.4），f11 已向回收（-0.5），f0 归零——
// 每轮净位移回零，循环无缝。
//
// 用法：node scripts/gen-tray-cat.mjs（可重跑，确定性渲染）
import { mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
import sharp from "sharp";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

// ---- 几何常量（32×24 网格；内部按「猫头朝左」建模，输出时整体镜像成头朝右）----
const VIEW_W = 32;
const VIEW_H = 24;

// 头放低、贴近躯干（参考图的头是「长在身上」的，不再高高抬起）
const HEAD = { x: 7.0, y: 9.0, r: 2.7 };
// 颈部椭圆：填掉「头球」与「身椭」相切处的凹口，让轮廓连续圆润
const NECK = { x: 9.8, y: 10.2, rx: 2.7, ry: 2.5, rot: 20 };
// 敦实躯干：水平微拱（参考图的背线），比旧版厚一圈
const BODY = { x: 14.6, y: 11.3, rx: 6.7, ry: 3.5, rot: 2 };
const TAIL_ROOT = { x: 20.4, y: 9.9 };
const HIP_FRONT = { x: 10.6, y: 12.6 };
const HIP_HIND = { x: 18.8, y: 13.0 };
const TAIL_W = 1.6;
const LEG_W_NEAR = 2.0;
const LEG_W_FAR = 1.75;

// 躯干俯仰枢轴（约在身体中段）
const PIVOT = { x: 13.4, y: 11.3 };

// ---- 12 帧前跃姿态（坐标均为网格绝对值，头朝左、向左前跃）----
// pitch：躯干+头+尾的整体俯仰（deg，正 = 头微抬）；dx/dy：整只猫的位移
// （负 dy = 升空）；foot：脚掌俯仰（0 = 平贴地面，正 = 脚尖下垂）；
// fN/fF/hN/hF：前近/前远/后近/后远脚掌落点。
//
// dy 走抛物线：起跳后 3→6 帧减速上升（顶点附近变化最小）、7→8 帧加速下落，
// 定速播放也能读出重力。落地后 9/10 帧是「前爪承重 → 后爪追上」的压重过程。
const POSES = [
  // f0 站立（= 静态图）：四脚落地，卷尾上扬
  { pitch: 0, dx: 0, dy: 0, tail: 0, foot: 0,
    fN: [9.6, 19.7], fF: [11.0, 19.1], hN: [19.9, 19.7], hF: [21.2, 19.1] },
  // f1 起势：近前脚离地，重心后坐下沉
  { pitch: 3, dx: 0.15, dy: 0.45, tail: 6, foot: 8,
    fN: [9.3, 18.4], fF: [11.0, 19.1], hN: [19.9, 19.7], hF: [21.2, 19.1] },
  // f2 蓄力到底：前脚收至胸前，后腿压到最扁（最深的蹲）
  { pitch: 5, dx: 0.2, dy: 0.85, tail: 12, foot: 24,
    fN: [8.6, 16.8], fF: [10.7, 18.6], hN: [20.1, 19.7], hF: [21.4, 19.2] },
  // f3 蹬地：后腿蹬直、身体开始离地，前脚向前探出
  { pitch: 2, dx: -0.35, dy: -0.35, tail: -6, foot: 30,
    fN: [7.6, 16.6], fF: [9.2, 16.9], hN: [21.6, 18.4], hF: [22.6, 18.0] },
  // f4 上升：身体拉平前窜，后腿仍在身后伸展
  { pitch: 0, dx: -1.1, dy: -1.35, tail: -14, foot: 28,
    fN: [6.8, 16.0], fF: [8.4, 16.1], hN: [22.6, 17.2], hF: [23.4, 16.9] },
  // f5 接近顶点：上升变慢，后腿开始回收
  { pitch: -1, dx: -1.8, dy: -2.1, tail: -16, foot: 22,
    fN: [6.2, 15.4], fF: [7.8, 15.5], hN: [22.4, 16.2], hF: [23.2, 15.8] },
  // f6 腾空顶点：四腿收拢，身体最舒展（悬停感来自 f5→f7 的高度差很小）
  { pitch: -2, dx: -2.1, dy: -2.15, tail: -12, foot: 14,
    fN: [6.4, 16.2], fF: [8.0, 16.2], hN: [21.8, 15.6], hF: [22.6, 15.2] },
  // f7 下落：前爪向前下方伸出准备着地，后腿仍收着
  { pitch: -3, dx: -2.3, dy: -1.55, tail: -6, foot: 8,
    fN: [6.8, 17.6], fF: [8.4, 17.3], hN: [21.4, 15.4], hF: [22.2, 15.0] },
  // f8 前爪触地：前爪踩住地面，后爪仍在空中（离地约 4.5 格）
  { pitch: -5, dx: -2.4, dy: -0.55, tail: 0, foot: 2,
    fN: [7.4, 19.7], fF: [8.9, 19.1], hN: [21.0, 15.4], hF: [21.8, 15.0] },
  // f9 重心前移：身体压向前爪，后腿向下摆、即将触地
  { pitch: -3, dx: -2.0, dy: 0.15, tail: 6, foot: 0,
    fN: [8.2, 19.7], fF: [9.6, 19.1], hN: [20.4, 16.6], hF: [21.4, 16.2] },
  // f10 后爪落地：四爪归位、重心压到最低（缓冲）
  { pitch: 1, dx: -1.2, dy: 0.55, tail: 10, foot: 0,
    fN: [9.0, 19.7], fF: [10.4, 19.1], hN: [19.9, 19.7], hF: [21.2, 19.1] },
  // f11 起身回位：抬起一点、尾巴回落，随后回到 f0（净位移归零）
  { pitch: 0, dx: -0.5, dy: 0.3, tail: 6, foot: 0,
    fN: [9.5, 19.7], fF: [10.9, 19.1], hN: [19.9, 19.7], hF: [21.2, 19.1] },
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

  // 尾巴：从臀后上扬、向外卷成开口朝内（朝身体）的圆钩——参考图的标志卷尾
  const tail =
    `M${TAIL_ROOT.x} ${TAIL_ROOT.y} ` +
    `C22.2 9.3, 23.3 7.9, 23.2 6.2 ` +
    `C23.1 4.5, 21.6 3.4, 20.1 4.0 ` +
    `C19.0 4.5, 18.5 5.7, 19.0 6.8`;

  // 耳：小三角耳带圆尖（参考图是小耳朵，贝塞尔收圆防 18pt 锯齿）
  const earL = `M4.7 8.0 C4.9 6.4, 5.2 5.3, 5.7 5.3 C6.3 5.5, 6.9 6.4, 7.2 7.6 Z`;
  const earR = `M7.4 7.5 C7.8 6.1, 8.3 5.0, 8.8 5.1 C9.2 5.4, 9.4 6.6, 9.3 7.9 Z`;

  return Buffer.from(
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${VIEW_W} ${VIEW_H}"
      fill="none" stroke="${color}" color="${color}"
      stroke-linecap="round" stroke-linejoin="round">
      <defs>
        <mask id="eye">
          <rect x="0" y="0" width="${VIEW_W}" height="${VIEW_H}" fill="#fff"/>
          <!-- 眼睛：透明镂空（模板图只认 alpha） -->
          <circle cx="6.4" cy="8.5" r="0.75" fill="#000"/>
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
          <ellipse cx="4.9" cy="10.0" rx="1.8" ry="1.3"
            transform="rotate(-6 4.9 10.0)" fill="${color}" stroke="none"/>
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
