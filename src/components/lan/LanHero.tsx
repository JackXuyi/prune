import { useI18n } from "../../i18n/store";
import { formatClock } from "../../lib/format";
import { kindLabel, type LanSnapshot } from "../../types/lan";
import { DotMatrix } from "../charts/DotMatrix";
import { RefreshIcon } from "../icons";

/**
 * 局域网 Hero（eul 重设计 · ports-eul-mockup.html 局域网子页 ②）：
 * 设备点阵（一设备一点）+ 类型图例；右侧网段 / 扫描口径块。
 *
 * 点色：绿=本机 / 蓝=网关 / 黄=有入站连接 / 灰=其他在线 / 淡化=离线遗留。
 * 「扫描局域网」是本页唯一执行入口——进入分段绝不自动扫描（权限弹窗纪律）。
 */
export function LanHero({
  snapshot,
  scanning,
  onScan,
  tone = "net",
}: {
  snapshot: LanSnapshot;
  scanning: boolean;
  onScan: () => void;
  /** 域色：net = 端口页局域网子页（青）；device = 设备页（品红，页面主角） */
  tone?: "net" | "device";
}) {
  const { t, locale } = useI18n();
  const domainColor = tone === "device" ? "var(--color-domain-device)" : "var(--color-domain-net)";
  const devices = snapshot.devices;
  const self = devices.filter((d) => d.isSelf).length;
  const router = devices.filter((d) => d.isGateway).length;
  const inbound = devices.filter((d) => !d.isSelf && !d.isGateway && d.inbound.length > 0).length;
  const online = devices.filter((d) => d.online).length;
  const offline = devices.length - online;
  const otherOnline = Math.max(0, online - self - router - inbound);

  const dots = devices.map((d) => ({
    tone: (d.isSelf
      ? "accent"
      : d.isGateway
        ? "info"
        : d.inbound.length > 0
          ? "warn"
          : "neutral") as "accent" | "info" | "warn" | "neutral",
    dim: !d.online,
    title: `${d.ip} · ${d.hostname ?? d.vendor ?? kindLabel(d.kind, locale)}${
      d.isNew ? ` · ${t("lan.topo.legendNew")}` : ""
    }`,
  }));

  const scanMs = (snapshot.scanMs / 1000).toFixed(1);

  return (
    <section
      className="relative flex h-[128px] shrink-0 items-center gap-4 overflow-hidden rounded-[16px] border px-[18px] py-3"
      style={{
        borderColor: `color-mix(in srgb, ${domainColor} 26%, transparent)`,
        backgroundImage: `linear-gradient(120deg, color-mix(in srgb, ${domainColor} 9%, transparent), color-mix(in srgb, ${domainColor} 2%, transparent) 46%, transparent 76%)`,
        backgroundColor: "var(--color-surface-raised)",
      }}
      title={t("lan.hero.tip")}
    >
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-2.5">
          <span className="font-mono text-[25px] leading-none font-semibold tabular-nums text-ink">
            {online}
          </span>
          <span className="min-w-0 truncate text-[10px] text-ink-faint">
            {t("lan.hero.leadNote", {
              iface: snapshot.iface ?? "—",
              cidr: snapshot.cidr ?? "—",
            })}
          </span>
        </div>
        <DotMatrix
          className="mt-2.5"
          dot={7}
          gap={5}
          max={48}
          ariaLabel={t("lan.hero.dotAria", { self, router, inbound, online: otherOnline, offline })}
          items={dots}
        />
        <div className="mt-2 flex flex-wrap gap-x-3 gap-y-1">
          <LegendDot swatch="var(--color-accent)" label={t("lan.hero.legendSelf")} value={self} />
          <LegendDot swatch="var(--color-info)" label={t("lan.hero.legendRouter")} value={router} />
          <LegendDot swatch="var(--color-warn)" label={t("lan.hero.legendInbound")} value={inbound} />
          <LegendDot swatch="var(--color-chip)" label={t("lan.hero.legendOnline")} value={otherOnline} />
          <LegendDot swatch="var(--color-hatch)" label={t("lan.hero.legendOffline")} value={offline} />
        </div>
      </div>

      {/* 网段 / 扫描口径块 + 唯一执行入口 */}
      <div className="flex w-[176px] shrink-0 flex-col gap-1.5">
        <div className="flex items-baseline justify-between text-[9.5px] text-ink-faint">
          <span>{t("lan.hero.iface")}</span>
          <b className="min-w-0 truncate pl-2 font-mono text-[10.5px] font-medium tabular-nums text-ink-dim">
            {snapshot.iface ? `${snapshot.iface}${snapshot.cidr ? ` · ${snapshot.cidr}` : ""}` : "—"}
          </b>
        </div>
        <div className="flex items-baseline justify-between text-[9.5px] text-ink-faint">
          <span>{t("lan.hero.lastScan")}</span>
          <b className="font-mono text-[10.5px] font-medium tabular-nums text-ink-dim">
            {formatClock(snapshot.at)} · {scanMs}s
          </b>
        </div>
        <div className="flex items-baseline justify-between text-[9.5px] text-ink-faint">
          <span>{t("lan.hero.method")}</span>
          <b className="pl-2 text-[10px] font-medium text-ink-dim">
            {snapshot.scanned > 0 ? t("lan.hero.methodBoth") : t("lan.hero.methodNeighbors")}
          </b>
        </div>
        <button
          type="button"
          onClick={onScan}
          disabled={scanning}
          title={t("lan.hero.scanTip")}
          className="mt-0.5 flex h-[26px] items-center justify-center gap-1.5 rounded-[8px] border border-hairline bg-chip text-[11px] text-ink-dim transition-colors hover:text-ink disabled:opacity-40"
        >
          <RefreshIcon size={11} className={scanning ? "animate-spin" : ""} />
          {scanning ? t("common.progress.scanning") : t("lan.scan")}
        </button>
      </div>
    </section>
  );
}

function LegendDot({
  swatch,
  label,
  value,
}: {
  swatch: string;
  label: string;
  value: number;
}) {
  return (
    <span className="flex items-center gap-1.5 text-[9.5px] text-ink-faint">
      <em className="size-[7px] shrink-0 rounded-[2px]" style={{ background: swatch }} aria-hidden />
      {label}
      <b className="font-mono text-[10.5px] font-medium tabular-nums text-ink-dim">{value}</b>
    </span>
  );
}
