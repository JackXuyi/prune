/**
 * 灵动岛（M15）。窗口 380×220 固定不动，只有内部形状在变 —— 见
 * docs/m15-dynamic-island-plan.md §5.1：窗口 resize 做展开动画会撕裂 + 裁切。
 *
 * 一条容易忽略的物理约束：屏幕顶部 33 pt 是**物理挖孔**，画在那里的像素永远
 * 看不见。所以折叠态 50 pt 高里，真正能用的只有下面 17 pt 的内容条。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

import { openMain } from "../lib/ipc";

type Phase = "idle" | "collapsed" | "expanded";

interface TopProc {
  name: string;
  cpu: number;
  pid: number;
}

interface CpuPayload {
  kind: "cpu";
  avg: number;
  peak: number;
  ratio: number;
  samples: number;
  notchH?: number;
  top: TopProc[];
}

/** 与 Rust `island_geo` 保持一致 */
const COLLAPSED_W = 229;
const COLLAPSED_H = 50;
const EXPANDED_W = 380;
const EXPANDED_H = 180;
/** 刘海高度兜底值，真实值随 payload 下发（不同机型不同） */
const DEFAULT_NOTCH_H = 33;

/** 出现后无人干预的存活时间 */
const AUTO_HIDE_MS = 6000;
/** 悬停多久才展开：防鼠标划过误触 */
const HOVER_IN_MS = 300;
/** 移出多久收起：留给用户从折叠条移到展开卡的时间 */
const HOVER_OUT_MS = 500;
/** 收起后多久真正隐藏 */
const AFTER_LEAVE_HIDE_MS = 2500;

const EASE = "cubic-bezier(0.32, 0.72, 0, 1)";

export function Island() {
  const [phase, setPhase] = useState<Phase>("idle");
  const [data, setData] = useState<CpuPayload | null>(null);
  const [notchH, setNotchH] = useState(DEFAULT_NOTCH_H);

  // listen 回调是长生命周期闭包，读 state 会拿到旧值，所以用 ref 镜像
  const phaseRef = useRef<Phase>("idle");
  const timers = useRef<Record<string, ReturnType<typeof setTimeout> | undefined>>({});

  const setPhaseBoth = (p: Phase) => {
    phaseRef.current = p;
    setPhase(p);
  };

  const clearTimer = (key: string) => {
    const t = timers.current[key];
    if (t) clearTimeout(t);
    timers.current[key] = undefined;
  };

  const setTimer = (key: string, fn: () => void, ms: number) => {
    clearTimer(key);
    timers.current[key] = setTimeout(fn, ms);
  };

  const hide = useCallback(() => {
    clearTimer("hide");
    clearTimer("expand");
    clearTimer("collapse");
    setPhaseBoth("idle");
    void invoke("island_hide");
  }, []);

  const expand = useCallback(() => {
    clearTimer("hide");
    // 先告诉 Rust 目标形状，再动画：否则命中测试还在用折叠矩形，
    // 鼠标一进展开区就被判成「离开」，刚展开就收起
    void invoke("island_set_expanded", { expanded: true });
    setPhaseBoth("expanded");
  }, []);

  const collapse = useCallback(() => {
    void invoke("island_set_expanded", { expanded: false });
    setPhaseBoth("collapsed");
    setTimer("hide", hide, AFTER_LEAVE_HIDE_MS);
  }, [hide]);

  useEffect(() => {
    let disposed = false;
    const disposers: Array<() => void> = [];

    const setups = [
      listen<CpuPayload>("island://alert", (e) => {
        const p = e.payload;
        setData(p);
        if (typeof p.notchH === "number" && p.notchH > 0) setNotchH(p.notchH);
        void invoke("island_set_expanded", { expanded: false });
        setPhaseBoth("collapsed");
        setTimer("hide", hide, AUTO_HIDE_MS);
      }),
      listen<boolean>("island://hover", (e) => {
        if (phaseRef.current === "idle") return;
        if (e.payload) {
          clearTimer("collapse");
          clearTimer("hide");
          setTimer("expand", expand, HOVER_IN_MS);
        } else {
          clearTimer("expand");
          if (phaseRef.current === "expanded") {
            setTimer("collapse", collapse, HOVER_OUT_MS);
          } else {
            setTimer("hide", hide, AFTER_LEAVE_HIDE_MS);
          }
        }
      }),
    ];

    // 两个监听都注册完才握手：panel 在 App 启动时就建好并隐藏，WKWebView 的加载
    // 时机不可控，第一次提报可能早于监听注册 —— 那时 emit 会被静默丢弃
    void Promise.all(setups).then((uns) => {
      if (disposed) {
        uns.forEach((un) => un());
        return;
      }
      disposers.push(...uns);
      void invoke("island_ready");
    });

    return () => {
      disposed = true;
      disposers.forEach((un) => un());
      Object.keys(timers.current).forEach(clearTimer);
    };
  }, [collapse, expand, hide]);

  const expanded = phase === "expanded";
  const visible = phase !== "idle";
  const w = expanded ? EXPANDED_W : COLLAPSED_W;
  const h = expanded ? EXPANDED_H : COLLAPSED_H;

  const onOpen = () => {
    void openMain(undefined, "stats");
    hide();
  };

  return (
    <div
      style={{
        width: "100vw",
        height: "100vh",
        display: "flex",
        justifyContent: "center",
        alignItems: "flex-start",
        background: "transparent",
        overflow: "hidden",
      }}
    >
      <div
        onContextMenu={(e) => {
          e.preventDefault();
          hide();
        }}
        onClick={onOpen}
        style={{
          width: w,
          height: visible ? h : 0,
          marginTop: 0,
          background: "#000",
          // 顶部保持方角：物理刘海本身是方缺口，圆上去就成了「悬浮黑盒子」
          borderRadius: "0 0 20px 20px",
          overflow: "hidden",
          position: "relative",
          cursor: "default",
          opacity: visible ? 1 : 0,
          transition: `width 260ms ${EASE}, height 260ms ${EASE}, opacity 160ms ${EASE}, border-radius 260ms ${EASE}`,
          animation: visible ? "island-in 220ms cubic-bezier(0.2, 0.9, 0.2, 1)" : undefined,
        }}
      >
        {phase === "collapsed" && data && (
          <div
            style={{
              position: "absolute",
              top: notchH,
              left: 0,
              right: 0,
              bottom: 0,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              gap: 8,
              color: "#fff",
            }}
          >
            <Bolt />
            <span
              style={{
                fontSize: 13,
                fontWeight: 500,
                fontVariantNumeric: "tabular-nums",
                letterSpacing: 0.2,
              }}
            >
              {Math.round(data.avg)}%
            </span>
          </div>
        )}

        {phase === "expanded" && data && (
          <div
            style={{
              position: "absolute",
              top: notchH,
              left: 0,
              right: 0,
              bottom: 0,
              padding: "12px 18px 16px",
              display: "flex",
              flexDirection: "column",
              color: "#fff",
              animation: "island-fade 180ms ease-out",
            }}
          >
            <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
              <Bolt />
              <span
                style={{
                  fontSize: 26,
                  fontWeight: 500,
                  fontVariantNumeric: "tabular-nums",
                  lineHeight: 1,
                }}
              >
                {Math.round(data.avg)}%
              </span>
              <span style={{ fontSize: 12, color: "rgba(255,255,255,0.55)" }}>
                CPU 持续偏高
              </span>
            </div>

            <div
              style={{
                marginTop: 6,
                fontSize: 11,
                color: "rgba(255,255,255,0.5)",
                fontVariantNumeric: "tabular-nums",
              }}
            >
              {data.samples} 个采样里 {Math.round(data.ratio)}% 超过阈值 · 峰值{" "}
              {Math.round(data.peak)}%
            </div>

            <div style={{ marginTop: 10, display: "flex", flexDirection: "column", gap: 5 }}>
              {data.top.length === 0 ? (
                <span style={{ fontSize: 11, color: "rgba(255,255,255,0.4)" }}>
                  明细稍后就绪
                </span>
              ) : (
                data.top.map((p) => (
                  <div
                    key={`${p.pid}-${p.name}`}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      fontSize: 11,
                      color: "rgba(255,255,255,0.72)",
                    }}
                  >
                    <span
                      style={{
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                        maxWidth: 250,
                      }}
                    >
                      {p.name}
                    </span>
                    <span style={{ fontVariantNumeric: "tabular-nums", color: "#FF9F0A" }}>
                      {Math.round(p.cpu)}%
                    </span>
                  </div>
                ))
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function Bolt() {
  return (
    <svg width="11" height="13" viewBox="0 0 11 13" fill="none" aria-hidden>
      <path d="M6.4 0.5 1 7.2h3.3L4.3 12.5 9.8 5.6H6.4L6.4 0.5Z" fill="#FF9F0A" />
    </svg>
  );
}
