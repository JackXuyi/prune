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
  const color =
    tone === "accent"
      ? "rgba(52, 211, 153, 0.75)"
      : tone === "warn"
        ? "rgba(245, 183, 74, 0.8)"
        : tone === "danger"
          ? "rgba(248, 113, 113, 0.85)"
          : tone === "mem"
            ? "rgba(106, 169, 255, 0.8)"
            : tone === "cpu"
              ? "rgba(167, 139, 250, 0.8)"
              : tone === "net"
                ? "rgba(34, 211, 238, 0.8)"
                : "rgba(255, 255, 255, 0.22)";

  return (
    <div className="h-[3px] w-full overflow-hidden rounded-full bg-white/[0.06]" aria-hidden>
      <div
        className="h-full rounded-full transition-[width] duration-500"
        style={{ width: `${width}%`, background: color }}
      />
    </div>
  );
}
