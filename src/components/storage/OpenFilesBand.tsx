import { useCallback, useEffect, useRef, useState } from "react";

import { useI18n } from "../../i18n/store";
import { formatRelative } from "../../lib/format";
import { isTauri, openFilesScan, openFilesVolumes } from "../../lib/ipc";
import type { OpenFilesReport } from "../../types/disk";
import { SearchIcon } from "../icons";

/**
 * 占用反查带（F23 · storage-eul-mockup.html ②b）：外置盘弹不出时问「谁占着」。
 *
 * eul 改版（用户定稿）：
 * - **上移到 Hero 正下方**——外置盘弹不出是高频痛点，不该埋在页尾；
 * - **路径 = 卷芯片点选，免输入**（openFilesVolumes 枚举）；点卷即查；
 * - 结果区默认收起、有命中才展开；任意路径走「其他路径…」兜底（可输入）。
 *
 * 三条纪律不变：只查不解（结果行无解除占用按钮）；coverage 永远展示
 * （空态是「没有可见的」≠「没人占用」）；lsof TTL 5 min（切 tab 不重跑）。
 */
export function OpenFilesBand() {
  const { t } = useI18n();
  const [volumes, setVolumes] = useState<string[]>([]);
  /** 选择值：空 = 未选；「其他路径…」手输也落这里 */
  const [selected, setSelected] = useState("");
  const [manual, setManual] = useState(false);
  const [report, setReport] = useState<OpenFilesReport | null>(null);
  const [scanning, setScanning] = useState(false);
  const [slow, setSlow] = useState(false);
  const [error, setError] = useState("");
  /** 扫描完成时刻（相对时间展示「N 分钟前」） */
  const [scannedAt, setScannedAt] = useState<number | null>(null);

  useEffect(() => {
    if (!isTauri()) return;
    void openFilesVolumes()
      .then((v) => {
        setVolumes(v);
        // 预选第一个卷但不自动跑 lsof：反查是显式动作，不替用户启动扫描
        if (v.length > 0) setSelected(v[0]);
        else setManual(true);
      })
      .catch(() => setManual(true));
  }, []);

  const scan = useCallback(async (path: string) => {
    const p = path.trim();
    if (!p || !isTauri()) return;
    setScanning(true);
    setError("");
    setSlow(false);
    const slowTimer = setTimeout(() => setSlow(true), 3000);
    try {
      const out = await openFilesScan(p);
      setReport(out);
      setScannedAt(Math.floor(Date.now() / 1000));
    } catch (e) {
      setError(String(e));
      setReport(null);
    } finally {
      clearTimeout(slowTimer);
      setScanning(false);
    }
  }, []);

  /** 点卷即查：芯片选中即触发（同一卷重复点 = 再反查，TTL 由后端管） */
  const pickVolume = (v: string) => {
    setSelected(v);
    setReport(null);
    void scan(v);
  };

  const manualRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (manual) manualRef.current?.focus();
  }, [manual]);

  return (
    <section
      className="shrink-0 rounded-[11px] border border-hairline-soft bg-surface-raised px-3 py-2"
      aria-label={t("disk.openFiles.title")}
    >
      <div className="flex items-center gap-2">
        <span
          className="grid size-[22px] shrink-0 place-items-center rounded-[7px] text-info"
          style={{ background: "color-mix(in srgb, var(--color-info) 14%, transparent)" }}
          aria-hidden
        >
          <SearchIcon size={12} />
        </span>
        <span className="min-w-0 flex-1">
          <span className="block text-[11px] font-medium text-ink">{t("disk.openFiles.title")}</span>
          <span className="block truncate text-[9px] text-ink-faint">{t("disk.tool.hint")}</span>
        </span>
        {scanning ? (
          <span className="shrink-0 text-[10px] text-ink-faint">{t("disk.openFiles.scanning")}</span>
        ) : null}
      </div>

      {/* 卷芯片：点选即查；「其他路径…」兜底展开手输 */}
      <div className="mt-1.5 flex flex-wrap items-center gap-1.5 border-t border-hairline-soft pt-1.5">
        {volumes.map((v) => {
          const on = selected === v && !manual;
          return (
            <button
              key={v}
              type="button"
              disabled={scanning}
              onClick={() => pickVolume(v)}
              title={t("disk.tool.pickTip", { path: v })}
              className={`rounded-full border px-2.5 py-[3px] font-mono text-[10px] transition-colors disabled:opacity-50 ${
                on
                  ? "border-info/55 bg-info/12 text-info"
                  : "border-hairline bg-chip text-ink-dim hover:text-ink"
              }`}
            >
              {v.replace(/^\/Volumes\//, "")}
            </button>
          );
        })}
        {volumes.length > 0 ? (
          <button
            type="button"
            disabled={scanning}
            onClick={() => setManual(true)}
            title={t("disk.tool.moreTip")}
            className={`rounded-full border px-2.5 py-[3px] text-[10px] transition-colors disabled:opacity-50 ${
              manual
                ? "border-info/55 bg-info/12 text-info"
                : "border-hairline bg-chip text-ink-dim hover:text-ink"
            }`}
          >
            {t("disk.tool.more")}
          </button>
        ) : null}
        {manual ? (
          <span className="flex items-center gap-1.5">
            <input
              ref={manualRef}
              value={selected}
              onChange={(e) => setSelected(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void scan(selected);
              }}
              placeholder="/Volumes/…"
              spellCheck={false}
              className="w-52 rounded-md border border-hairline bg-surface-sunk px-2 py-1 text-[11px] text-ink placeholder:text-ink-faint focus:border-accent/50 focus:outline-none"
              aria-label={t("disk.openFiles.pickVolume")}
            />
            <button
              type="button"
              disabled={scanning || !selected.trim()}
              onClick={() => void scan(selected)}
              className="shrink-0 rounded-md border border-hairline bg-chip px-2.5 py-1 text-[10.5px] text-ink-dim transition-colors hover:text-ink disabled:opacity-40"
            >
              {report && !scanning ? t("disk.openFiles.rescan") : t("disk.openFiles.action")}
            </button>
          </span>
        ) : null}
      </div>

      {/* 大目录预期管理：不做中途取消（kill 子进程会留半截结论），文案先到位 */}
      {slow ? (
        <p className="mt-1.5 text-[9.5px] text-ink-faint">{t("disk.openFiles.slowHint")}</p>
      ) : null}
      {error ? (
        <p className="mt-1.5 rounded-lg bg-danger/[0.08] px-2.5 py-1.5 text-[10px] text-danger">{error}</p>
      ) : null}

      {/* 结果区：有报告才展开（默认收起不留白） */}
      {report && !scanning ? (
        <div className="mt-1.5 border-t border-hairline-soft pt-1.5">
          <p className="text-[9px] text-ink-faint">
            {t("disk.openFiles.summary", {
              n: report.holders.length,
              path: report.path,
              ms: report.scannedMs,
              when: scannedAt ? formatRelative(scannedAt) : "",
            })}
          </p>
          {report.holders.length === 0 ? (
            <p className="mt-1 text-[10.5px] text-ink-dim">{t("disk.openFiles.none")}</p>
          ) : (
            <ul className="mt-0.5">
              {report.holders.map((h) => (
                <li
                  key={h.pid}
                  className="flex items-baseline justify-between gap-3 rounded-md px-1 py-[2px] transition-colors hover:bg-chip"
                >
                  <span className="min-w-0 truncate text-[10.5px] text-ink">
                    {h.comm}
                    <span className="ml-1.5 font-mono text-[9px] text-ink-faint">{h.pid}</span>
                  </span>
                  <span className="shrink-0 text-[9.5px] text-ink-faint" title={h.sampleFiles.join("\n")}>
                    {t("disk.openFiles.fileCount", { n: h.files })}
                  </span>
                </li>
              ))}
            </ul>
          )}
          {/* Coverage 纪律：盲区说明永远展示——空态 ≠ 没人占用 */}
          {report.coverageNote ? (
            <p className="mt-1 rounded-lg bg-warn/[0.07] px-2.5 py-1 text-[9px] leading-relaxed text-warn">
              {report.coverageNote}
            </p>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}
