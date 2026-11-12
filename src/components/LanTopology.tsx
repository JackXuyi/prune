import { useEffect, useMemo, useRef, useState } from "react";

import { DeviceKindIcon } from "./icons";
import { tileTone } from "./LanDeviceCard";
import { useI18n } from "../i18n/store";
import { kindLabel, type LanDevice } from "../types/lan";

/**
 * 局域网拓扑视图（M20 + M20-b，docs/m20-lan-topology-plan.md / m20b-topology-polish-plan.md）：
 * 零依赖自绘——SVG 边层 + 绝对定位 HTML 节点层。
 *
 * 布局是「一条水平脊柱 + 两侧分支」，不是在网关周围摊同心环：
 *
 *     连我的设备1-N —— 本机 —— 网关 —— 连网关的设备1-N
 *
 * 语义上就是一棵树：网关居中，本机挂在网关左侧，连我的设备排在本机更外侧、
 * 连网关的设备排在网关右侧。两侧再按列堆叠，列内按设备类型聚簇。
 * 这样「谁连谁」一眼可见，边也不用横穿别人的标签——原先的环没有归属信息，
 * 25 台时只能靠位置猜，且几何上必然叠压。
 *
 * 交互（spec §4–§5）：节点可拖拽（位置记忆、不回弹）、悬停高亮连带边、
 * 点击回调 onSelect（详情弹窗由 LanBoard 持有，跨视图共享）。
 */

/** 画布内边距：容节点半径（19）与呼吸 */
const MARGIN = 48;
/** 同列相邻节点的纵向间距（标签在侧面，纵向只需容下两个圆） */
const STACK_GAP = 58;
/** 脊柱上的间隔：网关 → 第一列 / 本机 → 第一列 */
const SPINE_GAP = 76;
/** 网关 → 本机 的距离：脊柱那一段要看得出来 */
const SELF_DIST = 150;
/** 列宽候选：要容下「圆 + 标签」（19 + 6 + 80 = 105），先试宽的，排不下再收窄 */
const COL_W_LIST = [140, 124] as const;
/** 每列台数候选：从 4 起试，排不下就加高（画布纵向增长比横向挤压可读） */
const ROWS_MIN = 4;
const ROWS_MAX = 10;
/** 画布最小高度 */
const CANVAS_H_MIN = 460;
/** 底部多留的余量：最下一行的 IP 行贴着画布边不好看 */
const BOTTOM_EXTRA = 26;
/** 拖拽夹紧：节点中心不允许拖出画布内边距（标签横向另算，见 LABEL_W） */
const CLAMP_X = 44;
const CLAMP_Y = 36;
/** 位移超过该值判定为拖拽，抑制紧随的 click（spec §5） */
const DRAG_THRESHOLD = 4;
/** 标签列宽：13 字符 IP 在 9.5px mono 下实测 74.4px，给到 80 才不触发省略号 */
const LABEL_W = 80;
/** 侧排标签的整体占位：圆半径 + 间距 + 标签宽（拟合时要算进画布宽度） */
const LABEL_SPAN = 19 + 6 + LABEL_W;
/**
 * 文字描边：用画布底色做一圈光晕，把从文字下面穿过的边线「抠掉」。
 * 节点是网格排布，20+ 台时任何直连边都会斜穿别人的标签——把线压在文字下
 * 不给个遮罩，读起来就是删除线。比给每个标签加实心底板更轻（不增加视觉块）。
 */
const HALO: React.CSSProperties = {
  textShadow:
    "0 0 3px var(--color-surface-sunk), 0 0 3px var(--color-surface-sunk), 0 0 3px var(--color-surface-sunk)",
};

interface Pos {
  x: number;
  y: number;
}

/** 中段省略：`HP7C4…67F7` 比尾截更有辨识度（M20-b §4） */
function midTruncate(s: string): string {
  if (s.length <= 12) return s;
  return `${s.slice(0, 5)}…${s.slice(-5)}`;
}

/** kind 聚簇排序：同类型相邻 → 域色在一列里连成段（M20-b §3） */
const KIND_ORDER = [
  "router",
  "computer",
  "phone",
  "tablet",
  "nas",
  "tv",
  "speaker",
  "printer",
  "camera",
  "iot",
  "unknown",
];

function byKindThenIp(a: LanDevice, b: LanDevice): number {
  // 离线一律排到组末：不挤占在线设备的位置（原先环版是「离线进最外环」的等价物）
  if (a.online !== b.online) return a.online ? -1 : 1;
  const ka = KIND_ORDER.indexOf(a.kind);
  const kb = KIND_ORDER.indexOf(b.kind);
  if (ka !== kb) return ka - kb;
  return a.ip < b.ip ? -1 : 1;
}

/** 均分成 k 列（前面的列多一台） */
function splitStacks(items: LanDevice[], k: number): LanDevice[][] {
  const out: LanDevice[][] = [];
  const base = Math.floor(items.length / k);
  const extra = items.length % k;
  let i = 0;
  for (let c = 0; c < k; c++) {
    const size = base + (c < extra ? 1 : 0);
    out.push(items.slice(i, i + size));
    i += size;
  }
  return out;
}

/**
 * 布局：`[连我的设备] —— 本机 —— 网关 —— [连网关的设备]`（M20-b §3 修订版）。
 *
 * 拟合顺序：每列台数从 4 起试，列宽从宽到窄试——排得开就用宽列，
 * 排不开收窄一档；收窄到底仍排不开就加高画布（多一台一行），上限 10 台/列。
 * 画布只在这两种情况下变大，跨渲染 / 跨轮次完全确定。
 */
function planLayout(
  devices: LanDevice[],
  w: number,
): { pos: Record<string, Pos>; canvasH: number; spine: { gw: Pos | null; me: Pos | null } } {
  const gw = devices.find((d) => d.isGateway) ?? null;
  const me = devices.find((d) => d.isSelf) ?? null;
  const others = devices.filter((d) => d !== gw && d !== me);

  // 没有网关时本机顶替中心位，其余设备一律挂它右侧（没有再分组的依据）
  const mineSet = new Set(gw ? others.filter((d) => d.inbound.length > 0).map((d) => d.ip) : []);
  const mine = others.filter((d) => mineSet.has(d.ip)).sort(byKindThenIp);
  const rest = others.filter((d) => !mineSet.has(d.ip)).sort(byKindThenIp);

  const half = w / 2 - MARGIN;
  // 侧排标签要算进半宽：最外一列的列心到画布边还得容下「半径 + 间距 + 标签宽」。
  // 列心 x = SPINE_GAP + cw*(k + 0.5)，k 取到最后一列。
  const rightNeed = (cr: number, cw: number) => SPINE_GAP + cw * (cr - 0.5) + LABEL_SPAN;
  const leftNeed = (cl: number, cw: number) =>
    (gw && me ? SELF_DIST : 0) + (cl > 0 ? SPINE_GAP + cw * (cl - 0.5) + LABEL_SPAN : 0);

  let rows = ROWS_MIN;
  let colW: number = COL_W_LIST[0];
  let colsR = 1;
  let colsL = 0;
  let fitted = false;
  for (rows = ROWS_MIN; rows <= ROWS_MAX && !fitted; rows++) {
    for (const cw of COL_W_LIST) {
      const cr = Math.max(1, Math.ceil(rest.length / rows));
      const cl = mine.length > 0 ? Math.ceil(mine.length / rows) : 0;
      if (rightNeed(cr, cw) <= half && leftNeed(cl, cw) <= half) {
        colW = cw;
        colsR = cr;
        colsL = cl;
        fitted = true;
        break;
      }
    }
  }
  if (!fitted) {
    // 极端密度：按最紧一档硬排（宁可挤，也不能把两个节点算到同一格）
    rows = ROWS_MAX;
    colW = COL_W_LIST[COL_W_LIST.length - 1];
    colsR = Math.max(1, Math.ceil(rest.length / rows));
    colsL = mine.length > 0 ? Math.ceil(mine.length / rows) : 0;
  }

  const rightStacks = splitStacks(rest, colsR);
  const leftStacks = colsL > 0 ? splitStacks(mine, colsL) : [];
  const maxCount = Math.max(
    1,
    ...rightStacks.map((s) => s.length),
    ...leftStacks.map((s) => s.length),
  );
  const canvasH = Math.max(CANVAS_H_MIN, 2 * MARGIN + maxCount * STACK_GAP + BOTTOM_EXTRA);

  const gwPos: Pos = { x: w / 2, y: canvasH / 2 };
  const mePos: Pos | null = me ? (gw ? { x: gwPos.x - SELF_DIST, y: gwPos.y } : gwPos) : null;

  const pos: Record<string, Pos> = {};
  if (gw) pos[gw.ip] = gwPos;
  if (me && mePos) pos[me.ip] = mePos;

  const placeStack = (stack: LanDevice[], x: number) => {
    stack.forEach((d, i) => {
      pos[d.ip] = { x, y: gwPos.y + (i - (stack.length - 1) / 2) * STACK_GAP };
    });
  };

  rightStacks.forEach((stack, k) => {
    placeStack(stack, gwPos.x + SPINE_GAP + colW * (k + 0.5));
  });
  if (mePos) {
    leftStacks.forEach((stack, k) => {
      placeStack(stack, mePos.x - SPINE_GAP - colW * (k + 0.5));
    });
  }

  return { pos, canvasH, spine: { gw: gw ? gwPos : null, me: mePos } };
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
  const { t, locale } = useI18n();
  const boxRef = useRef<HTMLDivElement | null>(null);
  const [width, setWidth] = useState(760);
  const [pos, setPos] = useState<Record<string, Pos>>({});
  const [hovered, setHovered] = useState<string | null>(null);
  const dragRef = useRef<DragState | null>(null);
  const suppressClick = useRef(false);
  /** 用户亲手拖过的节点：重扫后仍保住位置，没拖过的按新布局重排 */
  const dragged = useRef<Set<string>>(new Set());

  // 画布宽度实测（spec §2）：初始兜底 760，首帧测量前足够不炸
  useEffect(() => {
    const el = boxRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      // 视图切到卡片时本画布被 hidden（宽高 0）：不拿 0 去重算布局，否则位置会被算歪
      if (!el.clientWidth) return;
      setWidth(el.clientWidth);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const sig = devices.map((d) => d.ip).join(",");
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const plan = useMemo(() => planLayout(devices, width), [sig, width]);

  // 位置合并（spec §2）：只保用户拖过的 key，其余跟布局走 —— 新设备入网后能落进空位。
  // 窗口变窄 / 画布变矮时要把这些旧坐标夹回边界内，否则拖过的节点会留在画布外。
  useEffect(() => {
    setPos((prev) => {
      const merged = { ...plan.pos };
      for (const ip of dragged.current) {
        const p = prev[ip];
        if (!p || !(ip in merged)) continue;
        merged[ip] = {
          x: Math.min(Math.max(p.x, CLAMP_X), Math.max(CLAMP_X, width - CLAMP_X)),
          y: Math.min(Math.max(p.y, CLAMP_Y), Math.max(CLAMP_Y, plan.canvasH - CLAMP_Y)),
        };
      }
      return merged;
    });
  }, [plan, width]);

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
      if (d?.moved) {
        suppressClick.current = true;
        dragged.current.add(d.ip);
      }
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

  // 脉冲收敛（M20-b §6）：只让 firstSeen 最新的那一台动，其余新设备给静态 ring + badge
  const pulseIp = (() => {
    const news = devices.filter((d) => d.isNew);
    if (news.length === 0) return null;
    const newest = news.reduce((a, b) => (b.firstSeen > a.firstSeen ? b : a));
    return newest.firstSeen > 0 ? newest.ip : null;
  })();

  // 边集合（树形）：网关→本机、网关→连网关的设备、本机→连我的设备。
  // 不做网关→全量星形——那会让线横穿本机与别人的标签，也和「谁连谁」的语义不符。
  interface Edge {
    key: string;
    a: Pos;
    b: Pos;
    /** 起点 / 终点节点的半径：边两端各退这么多，线不穿进圆里 */
    ra: number;
    rb: number;
    /** accent = 本机「连了我」安全信号线 */
    tone: "plain" | "accent";
    dashed: boolean;
    /** 该边是否与当前 hover 的节点相连 */
    near: boolean;
  }
  /** 节点视觉半径：网关 23、普通 19、未知小灰点 10 */
  const radiusOf = (d: LanDevice) =>
    d.isGateway ? 23 : !d.isSelf && d.kind === "unknown" && !d.hostname ? 10 : 19;
  const edges: Edge[] = [];
  const mePos = plan.spine.me;
  const gwPos = plan.spine.gw;
  const anchor = gwPos ?? mePos;
  /** 脊柱的 x：节点在它左边就把标签挂左侧，右边反之 */
  const spineX = anchor?.x ?? width / 2;
  if (anchor) {
    if (mePos && gwPos) {
      // 网关—本机：脊柱上那一段
      edges.push({
        key: "spine",
        a: gwPos,
        b: mePos,
        ra: 23,
        rb: 19,
        tone: "plain",
        dashed: false,
        near: hovered === me?.ip || hovered === gw?.ip,
      });
    }
    for (const d of devices) {
      const p = pos[d.ip];
      if (!p || d === gw || d === me) continue;
      const fromMe = mePos && d.inbound.length > 0;
      edges.push({
        key: `e-${d.ip}`,
        a: fromMe ? mePos : anchor,
        b: p,
        ra: fromMe ? 19 : 23,
        rb: radiusOf(d),
        tone: fromMe ? "accent" : "plain",
        dashed: fromMe ? true : !d.online,
        near: hovered === d.ip || hovered === (fromMe ? me?.ip : (gw?.ip ?? me?.ip)),
      });
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

  return (
    <div className="mt-3">
      <div
        ref={boxRef}
        className="relative overflow-hidden rounded-[12px] border border-hairline-soft bg-surface-sunk"
        style={{
          height: plan.canvasH,
          backgroundImage: "radial-gradient(var(--color-hatch) 1px, transparent 1px)",
          backgroundSize: "16px 16px",
        }}
      >
        {/* 边层：三次贝塞尔从脊柱节点甩到叶子，pointer-events 关闭不拦节点事件 */}
        <svg className="pointer-events-none absolute inset-0 h-full w-full">
          {edges.map((e) => {
            const isAccent = e.tone === "accent";
            const hl = e.near && anyHover;
            // 边弱化（M20-b §7）：网格密度下边必须明显弱于文字，否则线会「划掉」标签。
            // 无 hover 时 0.35，hover 时非相关边再降到 0.18。
            const opacity = hl ? 1 : isAccent ? (anyHover ? 0.35 : 0.7) : anyHover ? 0.18 : 0.35;
            const dx = e.b.x - e.a.x;
            const dy = e.b.y - e.a.y;
            const len = Math.hypot(dx, dy) || 1;
            const ux = dx / len;
            const uy = dy / len;
            // 两端各退一个半径：线止于圆边界，不从节点中心穿出（原先会隔着标签画过去）
            const x1 = e.a.x + ux * e.ra;
            const y1 = e.a.y + uy * e.ra;
            const x2 = e.b.x - ux * e.rb;
            const y2 = e.b.y - uy * e.rb;
            const mx = (x1 + x2) / 2;
            const d = `M ${x1} ${y1} C ${mx} ${y1}, ${mx} ${y2}, ${x2} ${y2}`;
            return (
              <path
                key={e.key}
                d={d}
                fill="none"
                stroke={
                  hl
                    ? isAccent
                      ? "var(--color-accent)"
                      : "var(--color-ink-dim)"
                    : isAccent
                      ? "var(--color-accent)"
                      : "var(--color-hairline)"
                }
                strokeWidth={hl ? 1.8 : isAccent ? 1.4 : 1.2}
                strokeDasharray={isAccent ? "5 3" : e.dashed ? "4 3" : undefined}
                opacity={opacity}
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
          // 未知设备降为小灰点（M20-b §5）：没有身份的设备不该配图标和问号
          const tiny = !special && d.kind === "unknown" && !d.hostname;
          const box = tiny ? 26 : d.isGateway ? 46 : 38;
          const { tile, icon } = tileTone(d);
          const title = special
            ? d.isSelf
              ? t("lan.card.self")
              : t("lan.card.gateway")
            : d.hostname
              ? midTruncate(d.hostname.replace(/\.local\.?$/, ""))
              : d.kind === "unknown"
                ? null
                : kindLabel(d.kind, locale);
          const showTitle = special || isHover;
          /** 脊柱上的两个节点（网关 / 本机）：标签朝上，避免横向脊柱线穿过文字 */
          const onSpine = special;
          /** 侧排方向：节点在脊柱哪一侧，标签就挂到另一侧 */
          const labelSide = p.x >= spineX ? "right" : "left";
          return (
            <div
              key={d.ip}
              className="absolute cursor-grab select-none active:cursor-grabbing"
              style={{
                left: p.x,
                top: p.y,
                width: box,
                height: box,
                transform: "translate(-50%, -50%)",
                touchAction: "none",
                // 离线降透明度是刻意弱化；0.45 在浅色主题下 IP 行几乎读不出，取 0.55 折中
                opacity: dimmed ? 0.55 : 1,
                zIndex: isHover ? 10 : 1,
              }}
              onPointerDown={(e) => onNodeDown(e, d)}
              onClick={() => onNodeClick(d)}
              onPointerEnter={() => setHovered(d.ip)}
              onPointerLeave={() => setHovered((h) => (h === d.ip ? null : h))}
            >
              <div
                className={`relative h-full w-full transition-transform ${isHover ? "scale-[1.08]" : ""}`}
              >
                {pulseIp === d.ip ? (
                  // 扩散范围收在 tile 内：inset 25% 起，scale(2) 刚好铺满节点盒
                  <span className="absolute inset-[25%] animate-ping rounded-full bg-accent/30" />
                ) : null}
                {tiny ? (
                  <span className="absolute inset-0 flex items-center justify-center">
                    <span
                      className={`block size-[20px] rounded-full bg-chip ring-1 ${
                        d.isNew ? "ring-accent/60" : "ring-hairline"
                      }`}
                    />
                  </span>
                ) : (
                  <span
                    className={`flex h-full w-full items-center justify-center rounded-full ${tile} ${icon} ${
                      d.isGateway
                        ? "border border-accent/40"
                        : d.isSelf
                          ? "border border-dashed border-accent/40"
                          : dimmed
                            ? "border border-dashed border-hairline"
                            : "border border-hairline-soft"
                    }`}
                  >
                    <DeviceKindIcon kind={d.kind} size={d.isGateway ? 18 : 15} />
                  </span>
                )}
                {/* 非脉冲的新设备：静态 accent 描边，仍可一眼识别（M20-b §6） */}
                {d.isNew && pulseIp !== d.ip ? (
                  <span
                    className={`pointer-events-none absolute rounded-full border border-accent/50 ${
                      tiny ? "inset-[3px]" : "inset-0"
                    }`}
                  />
                ) : null}
                {d.isNew ? (
                  <span className="absolute -right-2 -top-1.5 rounded bg-accent/[0.12] px-1 py-px text-[9.5px] leading-tight text-accent">
                    {t("lan.card.badgeNew")}
                  </span>
                ) : null}
              </div>

              {/* 标签（M20-b §4）：默认只有一行半透明 IP；hover / 本机 / 网关才展开名称。
                  位置在节点「背向脊柱」的一侧：边从脊柱那侧进来，文字就不会被线穿过
                  （原先标签在圆下方，横向的边正好在标签高度上，读起来像删除线）。
                  绝对定位在 tile 之外——展开不会顶动圆点，拖拽位置也就不漂。 */}
              <span
                className={`pointer-events-none absolute flex flex-col leading-tight transition-opacity ${
                  onSpine
                    ? "bottom-full left-1/2 -translate-x-1/2 items-center"
                    : labelSide === "right"
                      ? "left-full top-1/2 ml-[6px] -translate-y-1/2 items-start text-left"
                      : "right-full top-1/2 mr-[6px] -translate-y-1/2 items-end text-right"
                }`}
                style={{
                  width: LABEL_W,
                  opacity: isHover || special ? 1 : anyHover ? 0.25 : 0.85,
                }}
              >
                {showTitle && title ? (
                  <span className="block max-w-full truncate text-[10.5px] text-ink" style={HALO}>
                    {title}
                  </span>
                ) : null}
                <span
                  className={`block max-w-full truncate font-mono text-[9.5px] tabular-nums ${
                    isHover || special ? "text-ink-dim" : "text-ink-faint"
                  }`}
                  style={HALO}
                >
                  {d.ip}
                </span>
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
