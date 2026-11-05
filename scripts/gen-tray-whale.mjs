// 生成「DeepSeek 鲸鱼娘 · 吃白米饭」托盘图标（M11，docs/m11-tray-icon-theme-plan.md）
//
//   src-tauri/icons/tray-whale/{macos,win}.png          静态图（= 第 0 帧，动画关闭时用）
//   src-tauri/icons/tray-anim/whale/{macos,win}/f0..11  12 帧「吃一口饭」循环
//
// 为什么要 12 帧而不是一张静态图：托盘图标同时是 CPU 表。筷子抬落的节奏
// 由 CPU 占用决定（见 tray.rs 的 anim_beat_ms）—— CPU 越忙吃得越急，
// 12 帧一轮 = 三口饭（每口 4 帧：碗里 → 抬起 → 送到嘴边 → 咽下）。
//
// 构图：上半是头像，下半是饭碗，右下斜插一双筷子。
//   - 头像裁到「头顶 → 下巴」，圆形遮罩 —— 方形贴图在菜单栏里像缩略图，圆形像头像徽章
//   - 饭碗有碗沿（椭圆）、碗身（梯形）、碗足（小方台），像碗而不像碟子
//   - 白饭凸出碗沿 1~2.5px，宽度随「吃到第几口」递减，第 12 帧见底后下一轮添满
//   - 筷尖只到右嘴角外侧，不穿过眼睛/嘴（48px 下穿脸会糊成一团黑）
//
// 用法：node scripts/gen-tray-whale.mjs（可重跑，确定性渲染）
import { mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
import sharp from "sharp";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SRC = path.join(root, "assets/tray-whale.png");

const S = 4; // 超采样倍数：矢量部分在 4x 画布上画，缩回目标尺寸时边缘更干净
const NAVY = "#22305A";
const BOWL_FILL = "#F2F7FF";
const BOWL_INNER = "#31477A";
const CHOP_COLOR = "#E8BF8C";

// 头像裁切框（源图 1024×1024 坐标）：从头顶头发到下巴，留一点肩膀
const FACE_BOX = { left: 345, top: 250, width: 420, height: 420 };
const FACE_D = 27; // 48 画布下的头像直径
const FACE_X = 10.5;
const FACE_Y = 1;

// 每帧：饭量（1 = 满碗，0 = 见底）、筷子状态（0 碗里 / 1 抬起 / 2 到嘴边）、头像纵向偏移
// 12 帧 = 三口，每口 4 拍（舀 → 抬 → 送 → 咽），第 12 帧碗见底后循环回满碗
const SEQ = [
  { rice: 1.0, chop: 0, dy: 0 },
  { rice: 1.0, chop: 1, dy: 0 },
  { rice: 1.0, chop: 2, dy: -1 },
  { rice: 0.78, chop: 1, dy: 1 },
  { rice: 0.78, chop: 0, dy: 0 },
  { rice: 0.78, chop: 1, dy: 0 },
  { rice: 0.78, chop: 2, dy: -1 },
  { rice: 0.5, chop: 1, dy: 1 },
  { rice: 0.5, chop: 0, dy: 0 },
  { rice: 0.5, chop: 1, dy: 0 },
  { rice: 0.5, chop: 2, dy: -1 },
  { rice: 0.2, chop: 1, dy: 1 },
];

async function faceLayer(size, dy) {
  const d = Math.round(FACE_D * (size / 48) * S);
  const raw = await sharp(SRC)
    .extract(FACE_BOX)
    .resize(d, d, { fit: "cover" })
    .ensureAlpha()
    .toBuffer();
  const mask = Buffer.from(
    `<svg xmlns="http://www.w3.org/2000/svg" width="${d}" height="${d}">` +
      `<circle cx="${d / 2}" cy="${d / 2}" r="${d / 2 - 1}" fill="#fff"/></svg>`,
  );
  return {
    buf: await sharp(raw).composite([{ input: mask, blend: "dest-in" }]).png().toBuffer(),
    left: Math.round(FACE_X * (size / 48) * S),
    top: Math.round((FACE_Y + dy) * (size / 48) * S),
  };
}

// 饭碗 + 白饭 + 筷子，坐标全部按 48 基准写，靠 viewBox 缩放到目标尺寸
function overlaySvg(size, riceRatio, chopState) {
  const px = size * S; // 画布像素
  const u = 48; // 设计基准宽度

  // 筷尖：碗里 → 抬起 → 右嘴角外侧（不越到脸中线，避免穿嘴）
  const tipY = [33.6, 26.6, 24.8][chopState];
  const tipX = [24.6, 29.2, 34.4][chopState];

  // 白饭：二次贝塞尔弧，弧顶高出碗沿 0.9~2.4px，宽度随饭量收窄
  // 饭堆：扁圆顶（两段二次贝塞尔），宽度/高度随剩余饭量收缩
  const apex = 29.6 - (1.1 + 1.9 * riceRatio);
  const heapW = 3.6 + 6.0 * riceRatio;

  const chopStroke = 0.55 + 0.9 * (size / 48);
  const chop = `
    <g stroke-linecap="round">
      <line x1="44" y1="47" x2="${tipX}" y2="${tipY}" stroke="${NAVY}" stroke-width="${(chopStroke * 2.1).toFixed(2)}"/>
      <line x1="44" y1="47" x2="${tipX}" y2="${tipY}" stroke="${CHOP_COLOR}" stroke-width="${chopStroke.toFixed(2)}"/>
      <line x1="41.4" y1="47.6" x2="${(tipX - 2.4).toFixed(2)}" y2="${(tipY + 1.3).toFixed(2)}" stroke="${NAVY}" stroke-width="${(chopStroke * 1.85).toFixed(2)}"/>
      <line x1="41.4" y1="47.6" x2="${(tipX - 2.4).toFixed(2)}" y2="${(tipY + 1.3).toFixed(2)}" stroke="${CHOP_COLOR}" stroke-width="${(chopStroke * 0.85).toFixed(2)}"/>
    </g>`;

  // 筷尖夹着的一小撮饭（舀饭的两拍才有）
  const bite =
    chopState === 0
      ? ""
      : `<circle cx="${(tipX + 0.4).toFixed(2)}" cy="${(tipY - 0.6).toFixed(2)}" r="1.9"
                 fill="#FFFFFF" stroke="${NAVY}" stroke-width="0.7"/>`;

  return Buffer.from(
    `<svg xmlns="http://www.w3.org/2000/svg" width="${px}" height="${px}" viewBox="0 0 ${u} ${u}">
      <!-- 碗足 -->
      <path d="M 16.6 40.2 Q 24 41.6 31.4 40.2 L 30.6 42.9 Q 24 44.3 17.4 42.9 Z"
            fill="${BOWL_FILL}" stroke="${NAVY}" stroke-width="1.3" stroke-linejoin="round"/>
      <!-- 碗身（顶边即碗沿） -->
      <path d="M 9.4 29.6 L 12 39.3 Q 24 42.9 36 39.3 L 38.6 29.6 Z"
            fill="${BOWL_FILL}" stroke="${NAVY}" stroke-width="1.5" stroke-linejoin="round"/>
      <!-- 碗里的白饭：碗沿上凸起的白弧，轮廓靠深蓝描边才在浅色菜单栏里站得住 -->
      <path d="M ${(24 - heapW).toFixed(2)} 29.9
               Q ${(24 - heapW * 0.62).toFixed(2)} ${apex.toFixed(2)} 24 ${apex.toFixed(2)}
               Q ${(24 + heapW * 0.62).toFixed(2)} ${apex.toFixed(2)} ${(24 + heapW).toFixed(2)} 29.9 Z"
            fill="#FFFFFF" stroke="${NAVY}" stroke-width="1.25" stroke-linejoin="round"/>
      ${bite}
      ${chop}
    </svg>`,
  );
}

async function renderFrame(size, frame) {
  const px = size * S;
  const face = await faceLayer(size, frame.dy);
  const buf = await sharp({
    create: { width: px, height: px, channels: 4, background: { r: 0, g: 0, b: 0, alpha: 0 } },
  })
    .composite([
      { input: face.buf, left: face.left, top: face.top },
      { input: overlaySvg(size, frame.rice, frame.chop), left: 0, top: 0 },
    ])
    .png()
    .toBuffer();
  return sharp(buf).resize(size, size, { fit: "cover" }).png().toBuffer();
}

const SETS = [
  { dir: "macos", size: 48 },
  { dir: "win", size: 32 },
];

for (const { dir, size } of SETS) {
  const staticPath = path.join(root, "src-tauri/icons/tray-whale", `${dir}.png`);
  await mkdir(path.dirname(staticPath), { recursive: true });
  await sharp(await renderFrame(size, SEQ[0])).toFile(staticPath);

  const animDir = path.join(root, "src-tauri/icons/tray-anim/whale", dir);
  await mkdir(animDir, { recursive: true });
  for (const [i, frame] of SEQ.entries()) {
    await sharp(await renderFrame(size, frame)).toFile(path.join(animDir, `f${i}.png`));
  }
  console.log(`  tray-whale/${dir}.png + tray-anim/whale/${dir}/f0..${SEQ.length - 1}.png (${size}×${size})`);
}

console.log(`鲸鱼娘「吃白米饭」图标已生成（${SEQ.length} 帧）。`);
