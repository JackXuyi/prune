// 生成「蓝鲸 · 游泳看 CPU」托盘图标（M13 引入，M17 起去除 DeepSeek 品牌皮）
//
//   src-tauri/icons/tray-whale-logo/{macos,win}.png              静态图（= 第 0 帧）
//   src-tauri/icons/tray-anim/whale-logo/{macos,win}/f0..7.png   8 帧游泳循环
//
// 注意：目录名 tray-whale-logo / whale-logo 是历史遗留（曾是 DeepSeek Logo 皮），
// Rust 侧 include_bytes! 引用这些路径，改目录要同步改 tray_icon.rs，这里不动。
//
// 为什么是 8 帧：托盘图标同时是 CPU 表。尾鳍摆动节奏由 CPU 占用决定
// （tray.rs 的 anim_beat_ms）——CPU 越忙游得越急，8 帧 = 一次完整尾鳍往复
// （下压 → 摆平 → 上扬 → 回位）。
//
// 构图：白色圆徽章托底（深浅色菜单栏都站得住）+ 普通蓝鲸（自然海蓝，
// 带背鳍 / 胸鳍 / 小喷水，不含任何品牌元素）。尾鳍是独立 <g>，按帧角度
// 旋转（rotate 绕尾根枢轴）；身体整体做 1px 纵向起伏。
//
// 用法：node scripts/gen-tray-whale.mjs（可重跑，确定性渲染）
import { mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
import sharp from "sharp";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const S = 4; // 超采样倍数：矢量在 4x 画布上画，缩回目标尺寸边缘更干净
const BLUE = "#4A90D9"; // 自然海蓝（普通蓝鲸体色，非品牌色）
const FIN = "#3B77BE"; // 深一档的鳍（压在肚皮上仍可辨）
const BELLY = "#BBD9F2"; // 浅一档的肚皮
const RING = "rgba(74,144,217,0.30)"; // 徽章外圈（浅描边，不抢主体）

// 每帧：尾鳍角度（deg，绕尾根枢轴）+ 身体纵向偏移（48 画布 px）
// 8 帧 = 一次完整尾鳍往复：0 → 下压(+16°) → 回中 → 上扬(−16°) → 回中
const SEQ = [
  { a: 0, dy: 0 },
  { a: 9, dy: -0.6 },
  { a: 16, dy: -1.0 },
  { a: 9, dy: -0.6 },
  { a: 0, dy: 0 },
  { a: -9, dy: 0.6 },
  { a: -16, dy: 1.0 },
  { a: -9, dy: 0.6 },
];

// 鲸身：头在左（圆润大弧），向右收窄到尾根（31.5, 24.5）
const BODY =
  "M 8.5 25.5 C 9 20, 15 16.5, 21.5 17.5 " +
  "C 26.5 18.3, 30 21, 31.5 24.5 " +
  "C 30.5 28.5, 25.5 32.5, 19 32.8 " +
  "C 13 33, 8 29.5, 8.5 25.5 Z";

// 尾鳍：双叶（上下两瓣），枢轴在尾根。旋转角由 SEQ[a] 给出
const TAIL =
  "M 31 24.5 C 33.5 22.5, 35 19.5, 35.5 16.5 " +
  "C 37.5 19, 38 22.5, 36.8 24.8 " +
  "C 38 27.2, 37.5 30.8, 35.5 33.2 " +
  "C 35 30.2, 33.5 26.8, 31 24.8 Z";

// 背鳍：立在身体上缘（x 20-26，顶点 y≈15），与身体同色连成剪影
const DORSAL =
  "M 20.5 18.6 C 21.6 15.6, 24.2 14.7, 26.2 15.7 " +
  "C 24.6 16.9, 23 17.9, 21.8 18.9 Z";

// 胸鳍：垂在肚皮下方的叶状鳍，深色压在肚皮上
const PECTORAL =
  "M 15.5 30.5 C 16.8 33.6, 19.4 35.4, 22.4 35.4 " +
  "C 20.6 33.4, 18.6 31.8, 17.2 30.1 Z";

// 头顶小喷水：三道扇形水线（克制的装饰，48px 下仍可辨）
const SPOUT =
  "M 12.5 16 C 12 14, 11 12.7, 9.7 11.9 " +
  "M 12.5 16 C 12.5 13.7, 13.1 12.1, 14.3 11.1 " +
  "M 12.5 16 C 13.3 14.3, 14.7 13.3, 16.5 13";

function frameSvg(size, { a, dy }) {
  const px = size * S;
  const u = 48;
  return Buffer.from(
    `<svg xmlns="http://www.w3.org/2000/svg" width="${px}" height="${px}" viewBox="0 0 ${u} ${u}">
      <!-- 白色圆徽章托底：深浅色菜单栏里都有稳定对比 -->
      <circle cx="24" cy="24" r="23" fill="#FFFFFF" stroke="${RING}" stroke-width="1"/>
      <g transform="translate(0 ${dy.toFixed(2)})">
        <!-- 背鳍（先画，被身体压住下缘，连成一体） -->
        <path d="${DORSAL}" fill="${BLUE}"/>
        <!-- 肚皮：浅蓝，用鲸身自身做裁剪，不越界 -->
        <clipPath id="body">
          <path d="${BODY}"/>
        </clipPath>
        <!-- 尾鳍（先画，压在身体下方衔接处更自然） -->
        <g transform="rotate(${a.toFixed(2)} 31.5 24.5)">
          <path d="${TAIL}" fill="${BLUE}"/>
        </g>
        <path d="${BODY}" fill="${BLUE}"/>
        <ellipse cx="21" cy="34" rx="14" ry="6" fill="${BELLY}" clip-path="url(#body)"/>
        <!-- 胸鳍：压在肚皮上，深一档才看得清 -->
        <path d="${PECTORAL}" fill="${FIN}"/>
        <!-- 眼睛 -->
        <circle cx="14" cy="23.5" r="1.3" fill="#FFFFFF"/>
        <!-- 喷水 -->
        <path d="${SPOUT}" fill="none" stroke="${BLUE}" stroke-width="1.1"
              stroke-linecap="round" opacity="0.85"/>
      </g>
    </svg>`,
  );
}

async function renderFrame(size, frame) {
  return sharp(await sharp(frameSvg(size, frame)).png().toBuffer())
    .resize(size, size, { fit: "cover" })
    .png()
    .toBuffer();
}

const SETS = [
  { dir: "macos", size: 48 },
  { dir: "win", size: 32 },
];

for (const { dir, size } of SETS) {
  const staticPath = path.join(root, "src-tauri/icons/tray-whale-logo", `${dir}.png`);
  await mkdir(path.dirname(staticPath), { recursive: true });
  await sharp(await renderFrame(size, SEQ[0])).toFile(staticPath);

  const animDir = path.join(root, "src-tauri/icons/tray-anim/whale-logo", dir);
  await mkdir(path.dirname(animDir), { recursive: true });
  for (const [i, frame] of SEQ.entries()) {
    await sharp(await renderFrame(size, frame)).toFile(path.join(animDir, `f${i}.png`));
  }
  console.log(
    `  tray-whale-logo/${dir}.png + tray-anim/whale-logo/${dir}/f0..${SEQ.length - 1}.png (${size}×${size})`,
  );
}

console.log(`蓝鲸「游泳看 CPU」图标已生成（${SEQ.length} 帧）。`);
