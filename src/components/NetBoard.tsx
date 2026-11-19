import { useState } from "react";

import { useI18n } from "../i18n/store";
import { MonitorPage } from "./net/MonitorPage";
import { TestPage } from "./net/TestPage";

/**
 * 网络 tab 壳：二级页切换（监测 / 测试）+ 副标。
 *
 * 2026-11-19 重设计：原 3 个二级页（监测 / 诊断 / 站点）收敛为 2 个——
 * 诊断页并入测试页（诊断 / 测速 / 站点都是手动发起的主动测试，语义同族；
 * 监测页全是被动实时读数，混入主动测试会让页面双重职责）。
 * 监测页内容见 net/MonitorPage，测试页见 net/TestPage。
 */
type NetSubPage = "monitor" | "test";

export function NetBoard({ onOpenResources }: { onOpenResources: () => void }) {
  const { t } = useI18n();
  const [page, setPage] = useState<NetSubPage>("monitor");

  return (
    /* 测试页是固定高度账（62+128+84+站点 flex），不滚动；监测页保持原有滚动 */
    <div
      className={`min-h-0 flex-1 px-5 py-4 ${
        page === "test" ? "flex flex-col overflow-hidden" : "overflow-x-hidden overflow-y-auto"
      }`}
    >
      <div className="flex shrink-0 items-center justify-between">
        <p className="text-[11px] text-ink-faint">
          {page === "monitor" ? t("net.subMonitor") : t("net.subTest")}
        </p>
        <Segmented page={page} onChange={setPage} />
      </div>

      {page === "monitor" ? <MonitorPage onOpenResources={onOpenResources} /> : <TestPage />}
    </div>
  );
}

/** 网络 tab 的二级页切换（监测 = 被动实时读数；测试 = 手动发起） */
function Segmented({
  page,
  onChange,
}: {
  page: NetSubPage;
  onChange: (p: NetSubPage) => void;
}) {
  const { t } = useI18n();
  const items: { key: NetSubPage; label: string }[] = [
    { key: "monitor", label: t("net.subpage.monitor") },
    { key: "test", label: t("net.subpage.test") },
  ];
  return (
    <div
      className="flex shrink-0 rounded-full bg-chip p-0.5"
      role="tablist"
      aria-label={t("net.subpage.aria")}
    >
      {items.map((it) => {
        const active = page === it.key;
        return (
          <button
            key={it.key}
            type="button"
            role="tab"
            aria-selected={active}
            onClick={() => onChange(it.key)}
            className={`rounded-full px-3 py-1 text-[11px] transition-colors ${
              active
                ? "bg-chip-hover font-medium text-ink"
                : "text-ink-faint hover:text-ink-dim"
            }`}
          >
            {it.label}
          </button>
        );
      })}
    </div>
  );
}
