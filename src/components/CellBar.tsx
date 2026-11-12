/**
 * 卡片内比例条的轨道（M10）。间距由调用方控制，这里只管「一根 3 px 的条」。
 *
 * 缺失值不画条——「没有数据」不能长得像「0」；分母为 0 时整列不画。
 */
export function CellBar({
  ratio,
  tone = "neutral",
  show = true,
}: {
  /** 0–1；调用方保证已除以分母 */
  ratio: number;
  /** UI v4：mem 蓝 / cpu 紫 / net 青 与域色 token 对齐 */
  tone?: "neutral" | "accent" | "warn" | "danger" | "mem" | "cpu" | "net";
  show?: boolean;
}) {
  if (!show) return <div className="h-[3px] w-full" aria-hidden />;

  const width = Math.max(0, Math.min(1, ratio)) * 100;
  // 颜色一律走 token：v5 之前这里是 rgba 硬编码，浅色主题下条会糊成一片灰
  const token =
    tone === "accent"
      ? "var(--color-accent)"
      : tone === "warn"
        ? "var(--color-warn)"
        : tone === "danger"
          ? "var(--color-danger)"
          : tone === "mem"
            ? "var(--color-info)"
            : tone === "cpu"
              ? "var(--color-domain-cpu)"
              : tone === "net"
                ? "var(--color-domain-net)"
                : "var(--color-ink-faint)";
  const color = `color-mix(in srgb, ${token} ${tone === "neutral" ? 45 : 80}%, transparent)`;

  return (
    <div className="h-[3px] w-full overflow-hidden rounded-full bg-chip" aria-hidden>
      <div
        className="h-full rounded-full transition-[width] duration-500"
        style={{ width: `${width}%`, background: color }}
      />
    </div>
  );
}
