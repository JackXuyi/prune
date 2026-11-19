import { useI18n } from "../../i18n/store";
import {
  canKillOwner,
  dualStack,
  exposureMeta,
  isSystemProcess,
  localUrl,
  ownersOf,
  primaryOf,
  serviceLabel,
  type PortBinding,
  type PortEntry,
  type ReachOutcome,
  type WhyKind,
} from "../../types/net";
import { probeKey, usePortStore } from "../../stores/portStore";
import { Badge } from "../primitives";

/**
 * 端口表一行（eul 重设计 · ports-eul-mockup.html ④）：
 * 端口号升到 15px mono 成第一视觉锚 → 徽标列（暴露 / 服务 ✓? / 漂移）→
 * 进程 → 项目 → 连接数（外部来源 accent）。白话解释 / 实测 / 入站明细
 * 收进行下展开区，不再五种颜色平铺。
 */

/** 表头与行共用同一网格列（列宽改动必须两处同步，抽成常量防漂移） */
export const PORT_ROW_COLS =
  "grid grid-cols-[72px_104px_200px_minmax(0,1fr)_74px] items-center gap-2.5";

/** F17 实测四态 → 文案。MessageKey 严格类型，动态拼接过不了编译，switch 罗列 */
function reachOutcomeLabel(
  outcome: ReachOutcome,
  t: ReturnType<typeof useI18n>["t"],
): string {
  switch (outcome.outcome) {
    case "connected":
      return t("ports.reach.connected");
    case "refused":
      return t("ports.reach.refused");
    case "timeout":
      return t("ports.reach.timeout");
    case "noroute":
      return t("ports.reach.noroute");
    case "noifaceaddr":
      return t("ports.reach.noifaceaddr");
    case "failed":
      return t("ports.reach.failedDetail", { detail: outcome.detail });
  }
}

/** F21 对端来源段 → 文案。MessageKey 严格类型，动态拼接过不了编译，switch 罗列 */
function peerScopeLabel(
  scope: "loopback" | "lan" | "public" | "unknown",
  t: ReturnType<typeof useI18n>["t"],
): string {
  switch (scope) {
    case "loopback":
      return t("ports.inbound.scope.loopback");
    case "lan":
      return t("ports.inbound.scope.lan");
    case "public":
      return t("ports.inbound.scope.public");
    case "unknown":
      return t("ports.inbound.scope.unknown");
  }
}

/** F16 结论类别 → 文字色调：系统服务 / 冲突是警告（别杀 / 别动，杀掉会复活），
 *  dev / known 是确认级结论，其余（free / timewait / unknown）中性 */
function whyToneClass(kind: WhyKind): string {
  switch (kind) {
    case "systemservice":
    case "conflict":
      return "text-warn";
    case "devserver":
    case "knownservice":
      return "text-accent";
    default:
      return "text-ink-dim";
  }
}

export function PortRow({
  entry,
  onOpenProcess,
  onKill,
}: {
  entry: PortEntry;
  onOpenProcess: (pid: number) => void;
  onKill: (binding: PortBinding, port: number, x: number, y: number) => void;
}) {
  const { t, locale } = useI18n();
  const primary = primaryOf(entry);
  const owners = ownersOf(entry);
  const exposure = exposureMeta(entry.exposure, locale);
  const system = owners.length > 0 && owners.every((b) => isSystemProcess(b.comm));
  const killable = primary != null && canKillOwner(primary);
  const url = localUrl(entry.port);
  const dual = dualStack(entry);

  // 服务类型：L1 探针结果（确认）优先于 L0 推断。键与 Rust 缓存同源。
  const key = probeKey(entry);
  const probeResult = usePortStore((s) => s.probeResults[key]);
  const probing = usePortStore((s) => s.probing[key]) ?? false;
  const probeError = usePortStore((s) => s.probeErrors[key]) ?? "";
  const probe = usePortStore((s) => s.probe);
  const serviceKind = probeResult?.kind ?? entry.service;

  const probeDetail = [probeResult?.detail, probeResult?.version].filter(Boolean).join(" ");
  const probeTitle = probeResult
    ? t("ports.service.hintConfirmed") + (probeDetail ? ` · ${probeDetail}` : "")
    : t("ports.service.hintInferred");

  // F17 暴露面实测：只对外部面（any / interface）的 TCP 端口给入口——
  // loopback 是「本机可达」的定义使然，测了没有信息量（f17 方案 S1）
  const reachEligible =
    entry.proto === "tcp" && (entry.exposure === "any" || entry.exposure === "interface");
  const reachKey = String(entry.port);
  const reachResult = usePortStore((s) => s.reachResults[reachKey]);
  const reaching = usePortStore((s) => s.reaching[reachKey]) ?? false;
  const reachError = usePortStore((s) => s.reachErrors[reachKey]) ?? "";
  const reach = usePortStore((s) => s.reach);

  // F16 白话解释：所有行都给入口（UDP / 内核持有 / 空闲端口也不例外）——
  // 空闲端口的「为什么」同样有信息量（TIME_WAIT / Coverage 盲区口径，S3）
  const whyKey = String(entry.port);
  const whyResult = usePortStore((s) => s.whyResults[whyKey]);
  const whying = usePortStore((s) => s.whying[whyKey]) ?? false;
  const whyError = usePortStore((s) => s.whyErrors[whyKey]) ?? "";
  const why = usePortStore((s) => s.why);

  // F21 入站来源：计数是口径，明细 ≤5 条给人看。外部来源（lan/public）才展开
  // 行下明细——loopback-only 只进 tooltip（自己 curl 自己不算新闻）
  const inbound = entry.inbound ?? null;
  const inboundTotal = inbound
    ? inbound.loopback + inbound.lan + inbound.public + inbound.unknown
    : 0;
  const inboundMore = inbound ? Math.max(0, inboundTotal - inbound.detail.length) : 0;
  const inboundExternal = inbound ? inbound.lan + inbound.public + inbound.unknown : 0;
  const inboundLine = inbound
    ? t("ports.inbound.line", {
        total: inboundTotal,
        loopback: inbound.loopback,
        lan: inbound.lan,
        public: inbound.public,
        unknownSuffix:
          inbound.unknown > 0
            ? ` · ${peerScopeLabel("unknown", t)} ${inbound.unknown}`
            : "",
      })
    : "";

  return (
    <li>
      <div
        onDoubleClick={() => {
          if (primary?.pid != null) onOpenProcess(primary.pid);
        }}
        onContextMenu={(e) => {
          if (!killable || !primary) return;
          e.preventDefault();
          onKill(primary, entry.port, e.clientX, e.clientY);
        }}
        title={killable ? t("ports.row.hintKillable") : t("ports.row.hintLocked")}
        className={`group ${PORT_ROW_COLS} rounded-md px-2 py-1.5 transition-colors hover:bg-chip`}
      >
        <span className="flex items-baseline gap-1">
          <span className="font-mono text-[15px] leading-none font-semibold tabular-nums text-ink">
            {entry.port}
          </span>
          <span className="text-[9px] uppercase text-ink-faint">{entry.proto}</span>
        </span>

        <span className="flex min-w-0 flex-col items-start gap-1">
          <Badge tone={exposure.tone} title={exposure.hint}>
            {exposure.label}
          </Badge>
          {serviceKind ? (
            <Badge
              tone={probeResult ? "accent" : "info"}
              title={probeTitle + (serviceKind === "mcp" ? ` ${t("ports.service.hintStdio")}` : "")}
            >
              {serviceLabel(serviceKind, locale)}
              {probeResult ? " ✓" : " ?"}
            </Badge>
          ) : null}
          {/* F22 基线漂移：变宽是安全信号（warn），新监听是信息信号（accent）。
              消失不进 UI（噪音远大于信息，f22 方案 §3） */}
          {entry.drift ? (
            <Badge
              tone={entry.drift === "widened" ? "warn" : "accent"}
              title={
                entry.drift === "widened"
                  ? t("ports.drift.widenedTitle")
                  : t("ports.drift.newTitle")
              }
            >
              {entry.drift === "widened"
                ? t("ports.drift.widened")
                : t("ports.drift.new")}
            </Badge>
          ) : null}
        </span>

        <span className="min-w-0">
          <span className="block truncate text-[11.5px] text-ink">
            {primary ? primary.comm : t("ports.row.kernelOwned")}
            {primary?.pid != null ? (
              <span className="ml-1.5 font-mono text-[10px] text-ink-faint">{primary.pid}</span>
            ) : null}
          </span>
          <span className="mt-0.5 flex items-center gap-1.5 text-[10px] text-ink-faint">
            {owners.length > 1 ? <span>{t("ports.row.owners", { n: owners.length })}</span> : null}
            {dual ? <span>IPv4+IPv6</span> : null}
            {system ? <span className="text-ink-faint">{t("ports.row.system")}</span> : null}
            {primary?.pid != null && entry.proto === "tcp" && !probeResult ? (
              <button
                type="button"
                disabled={probing}
                onClick={() => void probe(entry)}
                title={t("ports.service.hintInferred")}
                className="shrink-0 text-accent opacity-0 transition-opacity group-hover:opacity-100 disabled:opacity-60"
              >
                {probing ? t("ports.probe.running") : t("ports.probe.action")}
              </button>
            ) : null}
            {reachEligible ? (
              <button
                type="button"
                disabled={reaching}
                onClick={() => void reach(entry)}
                title={t("ports.reach.title")}
                className="shrink-0 text-accent opacity-0 transition-opacity group-hover:opacity-100 disabled:opacity-60"
              >
                {reaching ? t("ports.reach.running") : t("ports.reach.action")}
              </button>
            ) : null}
            <button
              type="button"
              disabled={whying}
              onClick={() => void why(entry)}
              title={t("ports.why.title")}
              className="shrink-0 text-accent opacity-0 transition-opacity group-hover:opacity-100 disabled:opacity-60"
            >
              {whying
                ? t("ports.why.running")
                : whyResult
                  ? t("ports.why.again")
                  : t("ports.why.action")}
            </button>
          </span>
        </span>

        <span className="min-w-0">
          {primary?.projectLabel ? (
            <>
              <span className="block truncate text-[11.5px] text-ink-dim">
                {primary.projectLabel}
              </span>
              <span
                className="mt-0.5 block truncate font-mono text-[9.5px] text-ink-faint"
                title={primary.project ?? ""}
              >
                {primary.project}
              </span>
            </>
          ) : (
            <span className="text-[10.5px] text-ink-faint">—</span>
          )}
        </span>

        <span
          className="flex items-center justify-end gap-1.5"
          title={inboundLine || undefined}
        >
          <span
            className={`font-mono text-[12.5px] leading-none font-semibold tabular-nums ${
              inboundExternal > 0 ? "text-accent" : "text-ink"
            }`}
          >
            {entry.conns == null ? "—" : entry.conns}
          </span>
          {primary?.pid != null ? (
            <button
              type="button"
              onClick={() => onOpenProcess(primary.pid!)}
              className="shrink-0 text-[10px] text-accent opacity-0 transition-opacity group-hover:opacity-100"
            >
              {t("ports.rowLocate")}
            </button>
          ) : null}
        </span>
      </div>

      {/* 内置解释（AirPlay 5000/7000）：命中才出现，不占位 */}
      {entry.note ? (
        <p className="mb-1 ml-2 pl-2 text-[10px] leading-snug text-warn">{entry.note}</p>
      ) : null}
      {probeError ? (
        <p className="mb-1 ml-2 pl-2 text-[10px] leading-snug text-warn">
          {t("ports.probe.failed")} · {probeError}
        </p>
      ) : null}
      {/* serverInfo 深展示（P2）：确认细节（MCP serverInfo / Server 头 / banner）摊开在行下，
          不只藏在 hover 里——名字 + 版本是用户判断「这是谁的服务」的关键证据 */}
      {probeResult && (probeResult.detail || probeResult.version) ? (
        <p className="mb-1 ml-2 pl-2 text-[10px] leading-snug text-accent">
          {t("ports.probe.confirmedLine", {
            detail: [probeResult.detail, probeResult.version].filter(Boolean).join(" "),
          })}
        </p>
      ) : null}
      {/* F17 实测结果：实测为主、推断面为副（冲突时实测更有信息量），口径 caveat 放 hover（S3） */}
      {reachResult ? (
        <p
          className="mb-1 ml-2 pl-2 text-[10px] leading-snug text-ink-dim"
          title={reachResult.caveat}
        >
          {t("ports.reach.result", {
            addr: reachResult.triedAddr ?? "—",
            label: reachOutcomeLabel(reachResult.outcome, t),
            ms: reachResult.elapsedMs,
          })}
          {reachResult.outcome.outcome !== "connected" ? (
            <span className="ml-1 text-ink-faint">
              {t("ports.reach.inferredStill", { exposure: exposure.label })}
            </span>
          ) : null}
        </p>
      ) : null}
      {reachError ? (
        <p className="mb-1 ml-2 pl-2 text-[10px] leading-snug text-warn">
          {t("ports.reach.failed")} · {reachError}
        </p>
      ) : null}
      {/* F16 白话解释：headline 按类别上色，证据行 + 指路压暗一级。
          文案来自核心层（CLI 决策 W1-DP-6 中文单语），与 CLI / MCP 同源同词 */}
      {whyResult ? (
        <div className="mb-1 ml-2 pl-2">
          <p className={`text-[10px] leading-snug ${whyToneClass(whyResult.kind)}`}>
            {whyResult.headline}
          </p>
          {whyResult.evidence.length > 0 ? (
            <ul className="mt-0.5">
              {whyResult.evidence.map((line, i) => (
                <li key={i} className="text-[9.5px] leading-snug text-ink-faint">
                  · {line}
                </li>
              ))}
            </ul>
          ) : null}
          {whyResult.actionHint ? (
            <p className="mt-0.5 text-[9.5px] leading-snug text-ink-faint">
              {whyResult.actionHint}
            </p>
          ) : null}
        </div>
      ) : null}
      {whyError ? (
        <p className="mb-1 ml-2 pl-2 text-[10px] leading-snug text-warn">
          {t("ports.why.failed")} · {whyError}
        </p>
      ) : null}
      {/* F21 入站来源明细：只在有外部来源（局域网/公网）时展开——
          loopback-only 是「自己访问自己」，tooltip 够了 */}
      {inbound && inboundExternal > 0 ? (
        <div className="mb-1 ml-2 pl-2">
          <p className="text-[10px] leading-snug text-accent">{inboundLine}</p>
          <ul className="mt-0.5">
            {inbound.detail.slice(0, 5).map((p) => (
              <li key={p.addr} className="font-mono text-[9.5px] leading-snug text-ink-faint">
                &lt;- {p.addr}（{peerScopeLabel(p.scope, t)}）
              </li>
            ))}
            {inboundMore > 0 ? (
              <li className="text-[9.5px] leading-snug text-ink-faint">
                {t("ports.inbound.more", { n: inboundMore })}
              </li>
            ) : null}
          </ul>
        </div>
      ) : null}
      {/* 开发服务（有 cwd 归属）才给 loopback 地址：GUI 应用那行贴个 URL 只是噪音 */}
      {primary?.project && entry.proto === "tcp" ? (
        <p className="mb-1 ml-2 pl-2 font-mono text-[9.5px] text-ink-faint">{url}</p>
      ) : null}
    </li>
  );
}
