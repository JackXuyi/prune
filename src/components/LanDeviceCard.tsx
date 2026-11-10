import { useEffect, useState } from "react";

import { CopyIcon, DeviceKindIcon } from "./icons";
import { writeClipboard } from "../lib/clipboard";
import { kindLabel, type LanDevice } from "../types/lan";

/**
 * 局域网设备卡（m18-lan-cards-redesign-plan.md §4）：
 * 图标 tile（域色 14% 底）+ 状态点 + IP mono 徽标 + 「连了我」warn 徽标
 * + services 标签；点击打开设备详情弹窗。
 *
 * 展示原则：hostname 命中 → 设备名为主标题；查不到 → 回退类型名，不编造。
 * 「连了我」命中时卡片加 warn 描边，安全信号前置。
 */
export function LanDeviceCard({
  d,
  dimmed = false,
  onSelect,
}: {
  d: LanDevice;
  dimmed?: boolean;
  onSelect: (d: LanDevice) => void;
}) {
  const hostname = d.hostname?.replace(/\.local\.?$/, "") ?? null;
  const title = d.isSelf
    ? "本机"
    : d.isGateway
      ? "网关"
      : (hostname ?? kindLabel(d.kind));
  const subtitle = d.vendor ?? (d.randomizedMac ? "随机 MAC" : null);
  const state = !d.online ? "不可达" : d.state === "stale" ? "最近在线" : "在线";
  const dot = !d.online
    ? "bg-ink-faint"
    : d.state === "stale"
      ? "bg-warn"
      : "bg-accent";
  const { tile, icon } = tileTone(d);
  const hasInbound = d.inbound.length > 0;

  return (
    <div
      onClick={() => onSelect(d)}
      title="点击查看设备详情"
      className={`cursor-pointer rounded-[10px] border bg-white/[0.03] px-3 py-2.5 transition-colors ${
        hasInbound
          ? "border-warn/[0.4] hover:bg-warn/[0.04]"
          : "border-hairline-soft hover:border-hairline hover:bg-white/[0.05]"
      } ${dimmed ? "opacity-55" : ""}`}
    >
      <div className="flex items-center gap-2.5">
        <span
          className={`flex size-[34px] shrink-0 items-center justify-center rounded-[9px] ${tile} ${icon}`}
        >
          <DeviceKindIcon kind={d.kind} size={17} />
        </span>
        <span className="min-w-0 flex-1">
          <span className="flex items-center gap-1.5">
            <span className="truncate text-[12px] leading-tight text-ink">
              {title}
            </span>
            {d.isSelf ? (
              <span className="shrink-0 rounded bg-accent/[0.12] px-1.5 py-px text-[9.5px] text-accent">
                自己
              </span>
            ) : null}
            {d.isGateway ? (
              <span className="shrink-0 rounded bg-accent/[0.12] px-1.5 py-px text-[9.5px] text-accent">
                上联
              </span>
            ) : null}
          </span>
          {subtitle ? (
            <span className="mt-0.5 block truncate text-[10px] leading-tight text-ink-faint">
              {subtitle}
            </span>
          ) : null}
        </span>
        <span
          aria-label={state}
          title={state}
          className={`size-[7px] shrink-0 rounded-full ${dot}`}
        />
      </div>

      <div className="mt-2 flex flex-wrap items-center gap-1.5">
        <span className="rounded-md bg-surface-overlay px-1.5 py-0.5 font-mono text-[10.5px] tabular-nums text-ink">
          {d.ip}
        </span>
        {hasInbound ? (
          <span className="rounded-md bg-warn/[0.12] px-1.5 py-0.5 text-[10px] font-medium text-warn">
            连了我 {d.inbound.slice(0, 2).join(" / ")}
            {d.inbound.length > 2 ? " …" : ""}
          </span>
        ) : null}
        {d.services.slice(0, 2).map((s) => (
          <span
            key={s}
            className="truncate rounded-md bg-white/[0.05] px-1.5 py-0.5 text-[10px] text-ink-dim"
          >
            {s}
          </span>
        ))}
        {d.services.length > 2 ? (
          <span className="text-[10px] text-ink-faint">
            +{d.services.length - 2}
          </span>
        ) : null}
      </div>
    </div>
  );
}

/** 设备类型 → 域色 tile（plan §4 映射表：低饱和域色 + 14% 透明度底） */
function tileTone(d: LanDevice): { tile: string; icon: string } {
  if (d.isSelf || d.isGateway || d.kind === "router") {
    return { tile: "bg-accent/[0.14]", icon: "text-accent" };
  }
  switch (d.kind) {
    case "computer":
    case "phone":
    case "tablet":
      return { tile: "bg-info/[0.14]", icon: "text-info" };
    case "nas":
      return { tile: "bg-[#22d3ee]/[0.14]", icon: "text-[#22d3ee]" };
    case "tv":
    case "speaker":
    case "camera":
    case "iot":
      return { tile: "bg-[#a78bfa]/[0.14]", icon: "text-[#a78bfa]" };
    default:
      return { tile: "bg-white/[0.05]", icon: "text-ink-dim" };
  }
}

/** 设备详情弹窗：点击卡片打开，Esc / 点遮罩 / 关闭按钮退出 */
export function LanDeviceDetailModal({
  d,
  onClose,
}: {
  d: LanDevice;
  onClose: () => void;
}) {
  const [copied, setCopied] = useState<"ip" | "mac" | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const hostname = d.hostname?.replace(/\.local\.?$/, "") ?? null;
  const title = d.isSelf
    ? "本机"
    : d.isGateway
      ? "网关"
      : (hostname ?? kindLabel(d.kind));
  const subtitle = d.vendor ?? (d.randomizedMac ? "随机 MAC" : null);
  const state = !d.online ? "不可达" : d.state === "stale" ? "最近在线" : "在线";
  const dot = !d.online
    ? "bg-ink-faint"
    : d.state === "stale"
      ? "bg-warn"
      : "bg-accent";
  const stateTone = !d.online
    ? "text-ink-faint"
    : d.state === "stale"
      ? "text-warn"
      : "text-accent";
  const { tile, icon } = tileTone(d);
  const hasInbound = d.inbound.length > 0;

  async function copy(text: string, key: "ip" | "mac") {
    const ok = await writeClipboard(text);
    if (ok) {
      setCopied(key);
      window.setTimeout(() => setCopied(null), 1200);
    }
  }

  return (
    <div
      className="fixed inset-0 z-40 bg-black/40"
      onClick={onClose}
      onContextMenu={(e) => {
        e.preventDefault();
        onClose();
      }}
    >
      <div className="fixed inset-0 z-50 flex items-center justify-center p-6">
        <div
          role="dialog"
          aria-label={`设备详情：${title}`}
          onClick={(e) => e.stopPropagation()}
          onContextMenu={(e) => e.stopPropagation()}
          className="w-[320px] rounded-xl border border-hairline bg-surface-raised p-4 shadow-2xl shadow-black/50"
        >
          {/* 头部：图标 + 名称 + 状态 */}
          <div className="flex items-center gap-3">
            <span
              className={`flex size-[40px] shrink-0 items-center justify-center rounded-[10px] ${tile} ${icon}`}
            >
              <DeviceKindIcon kind={d.kind} size={20} />
            </span>
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-1.5">
                <span className="truncate text-[13.5px] font-medium text-ink">
                  {title}
                </span>
                {d.isSelf ? (
                  <span className="shrink-0 rounded bg-accent/[0.12] px-1.5 py-px text-[9.5px] text-accent">
                    自己
                  </span>
                ) : null}
                {d.isGateway ? (
                  <span className="shrink-0 rounded bg-accent/[0.12] px-1.5 py-px text-[9.5px] text-accent">
                    上联
                  </span>
                ) : null}
              </div>
              {subtitle ? (
                <div className="mt-0.5 truncate text-[10.5px] text-ink-faint">
                  {subtitle}
                </div>
              ) : null}
            </div>
            <span className="flex shrink-0 items-center gap-1.5">
              <span className={`size-[7px] rounded-full ${dot}`} />
              <span className={`text-[11px] ${stateTone}`}>{state}</span>
            </span>
          </div>

          {/* 字段明细 */}
          <dl className="mt-3.5 space-y-2 border-t border-hairline-soft pt-3 text-[11px]">
            <DetailRow label="IP 地址">
              <span className="flex items-center gap-1.5">
                <span className="font-mono tabular-nums text-ink">{d.ip}</span>
                <CopyBtn
                  label={copied === "ip" ? "已复制" : "复制"}
                  onClick={() => void copy(d.ip, "ip")}
                />
              </span>
            </DetailRow>
            <DetailRow label="MAC 地址">
              {d.mac ? (
                <span className="flex items-center gap-1.5">
                  <span className="font-mono tabular-nums text-ink">
                    {d.mac}
                    {d.randomizedMac ? (
                      <span className="ml-1 font-sans text-[9.5px] text-ink-faint">
                        随机
                      </span>
                    ) : null}
                  </span>
                  <CopyBtn
                    label={copied === "mac" ? "已复制" : "复制"}
                    onClick={() => {
                      const mac = d.mac;
                      if (mac) void copy(mac, "mac");
                    }}
                  />
                </span>
              ) : (
                <span className="text-ink-faint">查不到（邻居表无记录）</span>
              )}
            </DetailRow>
            <DetailRow label="厂商">
              <span className={d.vendor ? "text-ink" : "text-ink-faint"}>
                {d.vendor ?? "未知"}
              </span>
            </DetailRow>
            <DetailRow label="设备类型">
              <span className="text-ink">{kindLabel(d.kind)}</span>
            </DetailRow>
            <DetailRow label="主机名">
              <span
                className={hostname ? "font-mono text-ink" : "text-ink-faint"}
              >
                {hostname ?? "未广播（mDNS 无记录）"}
              </span>
            </DetailRow>
            <DetailRow label="发现的服务">
              {d.services.length > 0 ? (
                <span className="flex flex-wrap justify-end gap-1">
                  {d.services.map((s) => (
                    <span
                      key={s}
                      className="rounded-md bg-white/[0.05] px-1.5 py-0.5 text-[10px] text-ink-dim"
                    >
                      {s}
                    </span>
                  ))}
                </span>
              ) : (
                <span className="text-ink-faint">未发现</span>
              )}
            </DetailRow>
            <DetailRow label="入站连接">
              {hasInbound ? (
                <span className="font-mono tabular-nums text-warn">
                  {d.inbound.join(" / ")}
                </span>
              ) : (
                <span className="text-ink-faint">无</span>
              )}
            </DetailRow>
          </dl>

          {d.randomizedMac ? (
            <p className="mt-3 rounded-lg bg-white/[0.03] px-2.5 py-1.5 text-[10px] leading-relaxed text-ink-faint">
              随机化 MAC：设备为防跟踪轮换硬件地址，厂商识别与跨轮次聚合（以 IP
              为准）均受影响。
            </p>
          ) : null}

          <div className="mt-3 flex items-center justify-between">
            <p className="text-[9.5px] text-ink-faint">
              Esc 或点空白处关闭 · 数据来自本轮扫描快照
            </p>
            <button
              type="button"
              onClick={onClose}
              className="rounded-full bg-white/[0.06] px-3 py-1 text-[10.5px] text-ink-dim transition-colors hover:bg-white/[0.1] hover:text-ink"
            >
              关闭
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

function DetailRow({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-start justify-between gap-3">
      <dt className="shrink-0 pt-0.5 text-ink-faint">{label}</dt>
      <dd className="min-w-0 text-right">{children}</dd>
    </div>
  );
}

function CopyBtn({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex shrink-0 items-center gap-1 rounded bg-white/[0.05] px-1.5 py-0.5 text-[10px] text-ink-dim transition-colors hover:bg-white/[0.1] hover:text-ink"
    >
      <CopyIcon size={10} />
      {label}
    </button>
  );
}
