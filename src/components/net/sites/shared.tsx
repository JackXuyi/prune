import { useI18n } from "../../../i18n/store";
import type { FailStage, SiteProbe } from "../../../types/net";
import { siteTone } from "../../../types/net";

/**
 * 站点检测的展示 helpers：卡片与详情弹窗共用，不各写一份。
 * 三条纪律（方案 §12）决定措辞：
 * - 只写观测事实（「TCP 无响应」），不写「被墙」；
 * - refused（4xx）是可达 → 标黄不标红；
 * - 速率为 null 时「样本不足」，不出现 0 KB/s。
 */

/** 状态点着色类（语义色，沿用 siteTone 三色板） */
export function statusDot(v: SiteProbe["verdict"]): string {
  const tone = siteTone(v);
  return tone === "accent"
    ? "bg-accent"
    : tone === "warn"
      ? "bg-warn"
      : tone === "danger"
        ? "bg-danger"
        : "bg-ink-faint";
}

export function statusTextClass(v: SiteProbe["verdict"]): string {
  const tone = siteTone(v);
  return tone === "accent"
    ? "text-accent"
    : tone === "warn"
      ? "text-warn"
      : tone === "danger"
        ? "text-danger"
        : "text-ink-dim";
}

/** 卡片正文一句话状态（细节进弹窗，卡片放不下 curl 原文） */
export function statusText(p: SiteProbe, t: ReturnType<typeof useI18n>["t"]): string {
  switch (p.verdict) {
    case "slow":
      return t("net.sites.slowChip", { stage: stageLabel(p.stage, t) });
    case "refused":
      return t("net.sites.refusedChip", { code: p.httpCode ?? "—" });
    case "stalled":
      return t("net.sites.summary.stalled");
    case "unreachable":
      return t("net.sites.unreachableChip", { stage: stageLabel(p.stage, t) });
    case "error":
      return t("net.sites.summary.error");
    default:
      return t("net.sites.summary.ok");
  }
}

/** 阶段名（只有名字，句子的主语在调用方） */
export function stageLabel(stage: FailStage | null, t: ReturnType<typeof useI18n>["t"]): string {
  switch (stage) {
    case "dns":
      return t("net.sites.stage.dns");
    case "tcp":
      return t("net.sites.stage.tcp");
    case "tls":
      return t("net.sites.stage.tls");
    case "ttfb":
      return t("net.sites.stage.ttfb");
    case "transfer":
      return t("net.sites.stage.transfer");
    default:
      return t("net.sites.stage.http");
  }
}

/** 失败/拒绝的文案：只描述断点，不推断原因 */
export function failureText(p: SiteProbe, t: ReturnType<typeof useI18n>["t"]): string {
  const sec = p.totalMs == null ? null : (p.totalMs / 1000).toFixed(1);
  const stage: FailStage | null = p.stage;
  switch (p.verdict) {
    case "stalled":
      return t("net.sites.row.stalled", { sec: sec ?? "—" });
    case "refused":
      return t("net.sites.row.refused", { code: p.httpCode ?? "—" });
    case "error":
      return t("net.sites.row.errorNote", { why: p.note ?? "—" });
    case "unreachable":
      if (stage === "dns") return t("net.sites.row.dnsFail");
      if (stage === "tls") {
        return p.tcpMs == null
          ? t("net.sites.row.tlsFailNoTcp")
          : t("net.sites.row.tlsFail", { tcp: p.tcpMs.toFixed(0) });
      }
      return t("net.sites.row.tcpFail", { sec: sec ?? "—" });
    default:
      return "—";
  }
}

/** 有阶段读数 = 画三段读数；unreachable/stalled 只给一句断点说明 */
export function hasReadings(p: SiteProbe): boolean {
  return p.ttfbMs != null && p.verdict !== "unreachable";
}

/** 拿没拿到任何阶段读数：`error` 全是 null，瀑布与读数表都无从画起 */
export function hasAnyStage(p: SiteProbe): boolean {
  return p.dnsMs != null || p.tcpMs != null;
}
