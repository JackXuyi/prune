/**
 * D1：应用图标。
 *
 * 三层兜底顺序：真实图标（data URL）→ 字母徽标（家族哈希渐变）→ 什么都没有时也画徽标。
 * **永远不显示占位灰图**：拿不到图标不是一种状态，只是没有图标。
 */
import { useMemo } from "react";

/** FNV-1a 64（与 Rust appicon::fnv1a 同算法，仅用于挑渐变槽位） */
function fnv1a(s: string): bigint {
  let hash = 0xcbf29ce484222325n;
  const prime = 0x100000001b3n;
  for (let i = 0; i < s.length; i += 1) {
    hash ^= BigInt(s.charCodeAt(i));
    hash = (hash * prime) & 0xffffffffffffffffn;
  }
  return hash;
}

/** 六组低饱和渐变（HSL 160–280，与六域色同色温；不抢域色本义） */
const PALETTES: readonly [string, string][] = [
  ["#8ecfba", "#4f9c82"],
  ["#8cc3d4", "#4f8fa4"],
  ["#9fb8e8", "#5f7fbb"],
  ["#a9adea", "#6d71b8"],
  ["#c0a7e8", "#8468b5"],
  ["#c9a6dd", "#8e6aa5"],
];

function badgeOf(family: string): { letter: string; from: string; to: string } {
  const ch = [...family.trim()][0] ?? "?";
  const letter = /[a-z]/i.test(ch) ? ch.toUpperCase() : ch;
  const idx = Number(fnv1a(family) % 6n);
  const [from, to] = PALETTES[idx] ?? PALETTES[0];
  return { letter, from, to };
}

export function AppIcon({
  family,
  icon,
  size = 28,
  className = "",
  title,
}: {
  family: string;
  /** `data:image/png;base64,...`；缺失走字母徽标 */
  icon?: string | null;
  size?: 28 | 64;
  className?: string;
  title?: string;
}) {
  const badge = useMemo(() => badgeOf(family), [family]);
  const radius = size >= 64 ? 15 : 7;

  return (
    <span
      className={`relative inline-grid shrink-0 place-items-center overflow-hidden ${className}`}
      style={{
        width: size,
        height: size,
        borderRadius: radius,
        background: `linear-gradient(145deg, ${badge.from}, ${badge.to})`,
        // 暗底防止深色图标糊在背景上（inset ring 而非 border，避免撑大盒子）
        boxShadow:
          "inset 0 0 0 1px var(--color-hairline), 0 1px 3px rgba(0,0,0,0.35)",
      }}
      title={title}
      aria-hidden
    >
      <span
        style={{
          fontSize: size >= 64 ? 26 : 13,
          fontWeight: 650,
          letterSpacing: "-0.02em",
          color: "#0b0e11",
        }}
      >
        {badge.letter}
      </span>
      {icon ? (
        <img
          src={icon}
          alt=""
          className="prune-anim prune-icon-in absolute inset-0"
          style={{ width: "100%", height: "100%", objectFit: "cover" }}
          draggable={false}
        />
      ) : null}
    </span>
  );
}
