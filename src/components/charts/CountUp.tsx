/**
 * 数字滚动（UI v4 §3.3）：主读数变化时的 count-up 动效。
 *
 * 尊重 prefers-reduced-motion：命中时直接跳到目标值，不做补间。
 * rAF 单实例挂载/清理，切 tab 卸载不会留悬挂帧。
 */
import { useEffect, useRef, useState } from "react";

export function CountUp({
  value,
  format,
  duration = 400,
  className = "",
}: {
  /** 目标数值（原始量纲，如字节数） */
  value: number;
  /** 渲染格式化（如 formatBytes） */
  format: (v: number) => string;
  duration?: number;
  className?: string;
}) {
  const [display, setDisplay] = useState(value);
  const fromRef = useRef(value);
  const rafRef = useRef(0);

  useEffect(() => {
    const reduced =
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduced || duration <= 0 || !Number.isFinite(value)) {
      fromRef.current = value;
      setDisplay(value);
      return;
    }
    const from = fromRef.current;
    const start = performance.now();
    const tick = (t: number) => {
      const p = Math.min(1, (t - start) / duration);
      // ease-out：先快后慢，数字「落定」的手感
      const eased = 1 - Math.pow(1 - p, 3);
      const v = from + (value - from) * eased;
      setDisplay(v);
      if (p < 1) {
        rafRef.current = requestAnimationFrame(tick);
      } else {
        fromRef.current = value;
      }
    };
    rafRef.current = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafRef.current);
  }, [value, duration]);

  return <span className={`prune-anim ${className}`}>{format(display)}</span>;
}
