// 从 Lucide 图标源（assets/scissors.svg）生成 Prune 的两套图标：
//   1. src-tauri/icons/tray.png   —— 菜单栏模板图标（纯黑 + 透明，配合 icon_as_template 适配深浅色）
//   2. assets/icon-source.png     —— 1024x1024 应用图标源，再由 `pnpm tauri icon` 生成全套
//
// 用法：node scripts/gen-icons.mjs && pnpm tauri icon assets/icon-source.png
import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
import sharp from "sharp";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const sourceSvg = await readFile(path.join(root, "assets/scissors.svg"), "utf8");

// 取出 Lucide <svg> 内部图形（circle / path），复用其 24x24 坐标系
const glyph = sourceSvg
  .replace(/^[\s\S]*?<svg[^>]*>/, "")
  .replace(/<\/svg>\s*$/, "")
  .trim();

const strokeAttrs =
  'fill="none" stroke-linecap="round" stroke-linejoin="round"';

// ---- 1. 菜单栏模板图标：黑色描边 + 透明背景，48px（22pt @2x 有余量） ----
const traySvg = `<svg xmlns="http://www.w3.org/2000/svg" width="48" height="48" viewBox="0 0 24 24" ${strokeAttrs} stroke="#000000" stroke-width="1.9">${glyph}</svg>`;

await sharp(Buffer.from(traySvg))
  .resize(48, 48, { fit: "contain", background: { r: 0, g: 0, b: 0, alpha: 0 } })
  .png()
  .toFile(path.join(root, "src-tauri/icons/tray.png"));

// ---- 2. 应用图标：深色圆角底 + emerald 剪刀，24 网格放大 25 倍居中 ----
const appSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#16191F"/>
      <stop offset="1" stop-color="#0A0C10"/>
    </linearGradient>
  </defs>
  <rect width="1024" height="1024" rx="228" fill="url(#bg)"/>
  <g transform="translate(212 212) scale(25)" ${strokeAttrs} stroke="#34D399" stroke-width="1.6">${glyph}</g>
</svg>`;

const iconSource = path.join(root, "assets/icon-source.png");
await writeFile(iconSource, await sharp(Buffer.from(appSvg)).png().toBuffer());

console.log("已生成:");
console.log("  src-tauri/icons/tray.png   (菜单栏模板图标)");
console.log("  assets/icon-source.png     (应用图标源 1024x1024)");
