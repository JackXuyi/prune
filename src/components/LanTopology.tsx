import { useEffect, useRef, useState } from "react";

import { DeviceKindIcon } from "./icons";
import { useI18n } from "../i18n/store";
import { kindLabel, type LanDevice } from "../types/lan";

/**
 * 局域网拓扑视图（M20，docs/m20-lan-topology-spec.md）：
 * 网关居中的确定性星形布局，零依赖自绘——SVG 边层 + 绝对定位 HTML 节点层。
 *
 * 交互（spec §4–§5）：节点可拖拽（位置记忆、不回弹）、悬停高亮连带边、
 * 点击回调 onSelect（详情弹窗由 LanBoard 持有，跨视图共享）。
 * 新设备 ping 脉冲；「连了我」accent 虚线；离线置灰虚线。
 *
 * 边界（spec §7）：无网关时本机顶替中心位；布局重算只补缺失的 ip——
 * 拖过的位置不覆盖，重扫后消失的设备残留 pos 不渲染（无害）。
 */

const CANVAS_H = 460;
const R_MIN = 130;
const R_MAX = 260;
/** 本机内圈偏移：网关正下方（spec §3） */
const SELF_OFFSET = 84;
/** 拖拽夹紧：节点中心不允许拖出画布内边距（spec §3） */
const CLAMP_X = 30;
const CLAMP_Y = 26;
/** 位移超过该值判定为拖拽，抑制紧随的 click（spec §5） */
const DRAG_THRESHOLD = 4;

interface Pos {
  x: number;
  y: number;
}

/** 确定性哈希抖动 ±10°（spec §3）：重渲染不跳位，打散均匀角度的机械感 */
function jitterRad(ip: string): number {
  let h = 0;
  for (let i = 0; i < ip.length; i++) h = (h * 31 + ip.charCodeAt(i)) | 0;
  const deg = ((h % 21) + 21) % 21 - 10;
  return (deg * Math.PI) / 180;
}

/** 初始布局（spec §3 伪码的直译） */
function initialLayout(devices: LanDevice[], w: number, h: number): Record<string, Pos> {
  const pos: Record<string, Pos> = {};
  const cx = w / 2;
  const cy = h * 0.46;
  const gw = devices.find((d) => d.isGateway);
  const me = devices.find((d) => d.isSelf);
  if (gw) pos[gw.ip] = { x: cx, y: cy };
  if (me) pos[me.ip] = gw ? { x: cx, y: cy + SELF_OFFSET } : { x: cx, y: cy };
  const rest = devices
    .filter((d) => d !== gw && d !== me)
    .sort((a, b) => (a.ip < b.ip ? -1 : 1));
  const R = Math.min(R_MAX, Math.max(R_MIN, Math.min(w, h) / 2 - 100));
  rest.forEach((d, i) => {
    const angle = (i / rest.length) * Math.PI * 2 - Math.PI / 2 + jitterRad(d.ip);
    pos[d.ip] = { x: cx + R * Math.cos(angle), y: cy + R * Math.sin(angle) };
  });
  return pos;
}

interface DragState {
  ip: string;
  /** pointerdown 时刻指针相对节点中心的偏移，move 时保持抓点不跳 */
  dx: number;
  dy: number;
  startX: number;
  startY: number;
  moved: boolean;
}

export function LanTopology({
  devices,
  onSelect,
}: {
  devices: LanDevice[];
  onSelect: (d: LanDevice) => void;
}) {
  const { t } = useI18n();
  const boxRef = useRef<HTMLDivElement | null>(null);
  const [size, setSize] = useState({ w: 760, h: CANVAS_H });
  const [pos, setPos] = useState<Record<string, Pos>>({});
  const [hovered, setHovered] = useState<string | null>(null);
  const dragRef = useRef<DragState | null>(null);
  const suppressClick = useRef(false);

  // 画布实测（spec §2）：初始兜底 760×460，首帧测量前足够不炸
  useEffect(() => {
    const el = boxRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      setSize({ w: el.clientWidth, h: el.clientHeight });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const sig = devices.map((d) => d.ip).join(",");

  // 位置合并（spec §2）：初始布局只补缺失的 ip，已拖过的 key 不覆盖。
  // sig 已覆盖 devices 内容变化，devices 不进依赖（数组引用每次渲染都变）。
  useEffect(() => {
    setPos((prev) => {
      const merged = initialLayout(devices, size.w, size.h);
      for (const [ip, p] of Object.entries(prev)) {
        if (ip in merged) merged[ip] = p;
      }
      return merged;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sig, size.w, size.h]);

  // window 级拖拽监听（spec §5）：挂载注册一次，不为每次拖拽 add/remove
  useEffect(() => {
    const move = (e: PointerEvent) => {
      const d = dragRef.current;
      const el = boxRef.current;
      if (!d || !el) return;
      if (
        !d.moved &&
        Math.abs(e.clientX - d.startX) < DRAG_THRESHOLD &&
        Math.abs(e.clientY - d.startY) < DRAG_THRESHOLD
      ) {
        return; // 微抖不算拖拽
      }
      d.moved = true;
      const r = el.getBoundingClientRect();
      setPos((p) => ({
        ...p,
        [d.ip]: {
          x: Math.min(Math.max(e.clientX - r.left - d.dx, CLAMP_X), r.width - CLAMP_X),
          y: Math.min(Math.max(e.clientY - r.top - d.dy, CLAMP_Y), r.height - CLAMP_Y),
        },
      }));
    };
    const up = () => {
      const d = dragRef.current;
      if (d?.moved) suppressClick.current = true;
      dragRef.current = null;
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
  }, []);

  const gw = devices.find((d) => d.isGateway);
  const me = devices.find((d) => d.isSelf);
  const anchor = gw ?? me;

  const onNodeDown = (e: React.PointerEvent, d: LanDevice) => {
    const el = boxRef.current;
    const p = pos[d.ip];
    if (!el || !p) return;
    const r = el.getBoundingClientRect();
    dragRef.current = {
      ip: d.ip,
      dx: e.clientX - r.left - p.x,
      dy: e.clientY - r.top - p.y,
      startX: e.clientX,
      startY: e.clientY,
      moved: false,
    };
  };

  const onNodeClick = (d: LanDevice) => {
    if (suppressClick.current) {
      suppressClick.current = false;
      return;
    }
    onSelect(d);
  };

  // 边集合（spec §4.3）：anchor→设备为主边；inbound 命中追加本机 accent 虚线
  interface Edge {
    key: string;
    a: Pos;
    b: Pos;
    /** accent = 本机「连了我」安全信号线 */
    tone: "plain" | "accent";
    dashed: boolean;
    connectedToHover: boolean;
  }
  const edges: Edge[] = [];
  for (const d of devices) {
    const p = pos[d.ip];
    if (!p || d === gw || !anchor || d === anchor) continue;
    const ap = pos[anchor.ip];
    if (!ap) continue;
    edges.push({
      key: `link-${d.ip}`,
      a: ap,
      b: p,
      tone: "plain",
      dashed: !d.online,
      connectedToHover: hovered === d.ip || hovered === anchor.ip,
    });
    if (me && d !== me && d.inbound.length > 0) {
      const mp = pos[me.ip];
      if (mp) {
        edges.push({
          key: `inbound-${d.ip}`,
          a: mp,
          b: p,
          tone: "accent",
          dashed: true,
          connectedToHover: hovered === d.ip || hovered === me.ip,
        });
      }
    }
  }
  const anyHover = hovered !== null;

  // 图例（spec §4.4）：只在对应元素存在时渲染，不让图例撒谎
  const legend: { kind: "node" | "line"; label: string; cls: string }[] = [];
  if (me) legend.push({ kind: "node", label: t("lan.topo.legendSelf"), cls: "self" });
  if (gw) legend.push({ kind: "node", label: t("lan.topo.legendGateway"), cls: "gateway" });
  if (devices.some((d) => d.isNew))
    legend.push({ kind: "node", label: t("lan.topo.legendNew"), cls: "new" });
  if (devices.some((d) => !d.online))
    legend.push({ kind: "line", label: t("lan.topo.legendOffline"), cls: "offline" });
  if (me && devices.some((d) => d !== me && d.inbound.length > 0))
    legend.push({ kind: "line", label: t("lan.topo.legendInbound"), cls: "inbound" });

  const nodeTitle = (d: LanDevice) =>
    d.isSelf
      ? t("lan.card.self")
      : d.isGateway
        ? t("lan.card.gateway")
        : (d.hostname ?? kindLabel(d.kind));

  return (
    <div className="mt-3">
      <div
        ref={boxRef}
        className="relative overflow-hidden rounded-[12px] border border-hairline-soft bg-[#0b0e11]"
        style={{
          height: CANVAS_H,
          backgroundImage: "radial-gradient(rgba(255,255,255,0.05) 1px, transparent 1px)",
          backgroundSize: "16px 16px",
        }}
      >
        {/* 边层：SVG 直出画布坐标，pointer-events 关闭不拦节点事件 */}
        <svg className="pointer-events-none absolute inset-0 h-full w-full">
          {edges.map((e) => {
            const dim = anyHover && !e.connectedToHover;
            const isAccent = e.tone === "accent";
            const hl = e.connectedToHover && anyHover;
            return (
              <line
                key={e.key}
                x1={e.a.x}
                y1={e.a.y}
                x2={e.b.x}
                y2={e.b.y}
                stroke={
                  hl
                    ? isAccent
                      ? "var(--color-accent)"
                      : "rgba(255,255,255,0.4)"
                    : isAccent
                      ? "var(--color-accent)"
                      : "rgba(255,255,255,0.15)"
                }
                strokeWidth={hl ? 1.8 : isAccent ? 1.4 : 1.2}
                strokeDasharray={isAccent ? "5 3" : e.dashed ? "4 3" : undefined}
                opacity={dim ? 0.35 : e.dashed && !isAccent ? 0.5 : 1}
              />
            );
          })}
        </svg>

        {/* 节点层：HTML 复用 icon 与 tailwind token（spec §4.2） */}
        {devices.map((d) => {
          const p = pos[d.ip];
          if (!p) return null;
          const special = d.isSelf || d.isGateway;
          const isHover = hovered === d.ip;
          const dimmed = !d.online;
          return (
            <div
              key={d.ip}
              className="absolute flex w-[110px] cursor-grab flex-col items-center select-none active:cursor-grabbing"
              style={{
                left: p.x,
                top: p.y,
                transform: "translate(-50%, -50%)",
                touchAction: "none",
                opacity: dimmed ? 0.45 : 1,
                zIndex: isHover ? 10 : 1,
              }}
              onPointerDown={(e) => onNodeDown(e, d)}
              onClick={() => onNodeClick(d)}
              onPointerEnter={() => setHovered(d.ip)}
              onPointerLeave={() => setHovered((h) => (h === d.ip ? null : h))}
            >
              <div
                className={`relative transition-transform ${isHover ? "scale-[1.08]" : ""}`}
              >
                {d.isNew ? (
                  <span className="absolute inset-0 animate-ping rounded-full bg-accent/30" />
                ) : null}
                <span
                  className={`flex items-center justify-center rounded-full ${
                    d.isGateway ? "size-[54px]" : "size-[46px]"
                  } ${
                    special
                      ? "border bg-accent/[0.14] text-accent"
                      : "border border-hairline bg-white/[0.05] text-ink-dim"
                  } ${
                    d.isGateway
                      ? "border-accent/40"
                      : d.isSelf
                        ? "border-dashed border-accent/40"
                        : dimmed
                          ? "border-dashed"
                          : ""
                  }`}
                >
                  <DeviceKindIcon kind={d.kind} size={d.isGateway ? 19 : 17} />
                </span>
                {d.isNew ? (
                  <span className="absolute -right-1.5 -top-1 rounded bg-accent/[0.12] px-1 py-px text-[9.5px] leading-tight text-accent">
                    {t("lan.card.badgeNew")}
                  </span>
                ) : null}
              </div>
              <span className="mt-1 block w-full truncate text-center text-[11px] leading-tight text-ink">
                {nodeTitle(d)}
              </span>
              <span className="block w-full truncate text-center font-mono text-[10px] leading-tight tabular-nums text-ink-faint">
                {d.ip}
              </span>
            </div>
          );
        })}
      </div>

      {/* 提示 + 图例行（spec §4.4） */}
      <div className="mt-2 flex flex-wrap items-center justify-between gap-2">
        <p className="text-[10px] text-ink-faint">{t("lan.topo.dragHint")}</p>
        <div className="flex flex-wrap items-center gap-2.5">
          {legend.map((l) => (
            <span key={l.label} className="flex items-center gap-1 text-[10.5px] text-ink-faint">
              {l.kind === "node" ? (
                <span
                  className={`inline-block size-[7px] rounded-full ${
                    l.cls === "new"
                      ? "bg-accent/30 ring-1 ring-accent/60"
                      : "bg-accent/[0.14] ring-1 ring-accent/60"
                  }`}
                />
              ) : (
                <span
                  className={`inline-block h-0 w-4 border-t ${
                    l.cls === "inbound"
                      ? "border-accent border-dashed"
                      : "border-hairline border-dashed opacity-60"
                  }`}
                />
              )}
              {l.label}
            </span>
          ))}
        </div>
      </div>
    </div>
  );
}
