/**
 * F23 占用反查卡（docs/f23-open-files-plan.md）：谁占着这块盘。
 *
 * 外置盘弹出失败时 macOS 只说「有文件正在使用」——这张卡回答「是谁」。
 * 三条纪律的可视化：
 * 1. **只查不解**：占用者行只有「定位」入口（只看不执行），没有任何
 *    解除占用 / 强制弹出按钮——那是用户的决定，不是工具的刀；
 * 2. **Coverage 诚实**：coverageNote 永远随结果展示，空态是「没有可见的」
 *    而不是「没人占用」；
 * 3. **用户显式触发**：点「反查」才跑 lsof；Rust 侧 TTL 5 min，切 tab 不重跑。
 */
import { useCallback, useEffect, useState } from "react";

import { useI18n } from "../i18n/store";
import { formatRelative } from "../lib/format";
import { isTauri, openFilesScan, openFilesVolumes } from "../lib/ipc";
import type { OpenFilesReport } from "../types/disk";
import { Btn } from "./primitives";

/** 扫描耗时超过这个值前给出预期文案（Time Machine 级目录十余秒是正常态） */
const SLOW_AFTER_MS = 3000;

export function OpenFilesCard() {
  const { t } = useI18n();
  const [volumes, setVolumes] = useState<string[]>([]);
  /** 选择值：空 = 未选；手输路径也落这里 */
  const [selected, setSelected] = useState("");
  const [manual, setManual] = useState(false);
  const [report, setReport] = useState<OpenFilesReport | null>(null);
  const [scanning, setScanning] = useState(false);
  const [slow, setSlow] = useState(false);
  const [error, setError] = useState("");
  /** 扫描完成时刻（相对时间展示「缓存 N 分钟前」） */
  const [scannedAt, setScannedAt] = useState<number | null>(null);

  useEffect(() => {
    if (!isTauri()) return;
    void openFilesVolumes()
      .then((v) => {
        setVolumes(v);
        if (v.length > 0) setSelected(v[0]);
        else setManual(true);
      })
      .catch(() => setManual(true));
  }, []);

  const scan = useCallback(async () => {
    const path = selected.trim();
    if (!path || scanning || !isTauri()) return;
    setScanning(true);
    setError("");
    setSlow(false);
    const slowTimer = setTimeout(() => setSlow(true), SLOW_AFTER_MS);
    try {
      const out = await openFilesScan(path);
      setReport(out);
      setScannedAt(Math.floor(Date.now() / 1000));
    } catch (e) {
      setError(String(e));
    } finally {
      clearTimeout(slowTimer);
      setScanning(false);
    }
  }, [selected, scanning]);

  return (
    <section
      className="rounded-card border border-hairline bg-surface-raised px-4 py-3.5"
      aria-label={t("disk.openFiles.title")}
    >
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-baseline gap-2">
          <span className="text-[12px] font-medium">{t("disk.openFiles.title")}</span>
          <span className="text-[10px] text-ink-faint">{t("disk.openFiles.tag")}</span>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          {manual ? (
            <input
              value={selected}
              onChange={(e) => setSelected(e.target.value)}
              placeholder="/Volumes/…"
              spellCheck={false}
              className="w-52 rounded-md border border-hairline bg-surface-sunk px-2 py-1 text-[11px] text-ink placeholder:text-ink-faint focus:border-accent/50 focus:outline-none"
            />
          ) : (
            <select
              value={selected}
              onChange={(e) => {
                setSelected(e.target.value);
                setReport(null);
              }}
              className="rounded-md border border-hairline bg-surface-sunk px-2 py-1 text-[11px] text-ink focus:border-accent/50 focus:outline-none"
              aria-label={t("disk.openFiles.pickVolume")}
            >
              {volumes.map((v) => (
                <option key={v} value={v}>
                  {v.replace(/^\/Volumes\//, "")}
                </option>
              ))}
            </select>
          )}
          <Btn onClick={() => void scan()} disabled={scanning || !selected.trim()}>
            {scanning
              ? t("disk.openFiles.scanning")
              : report
                ? t("disk.openFiles.rescan")
                : t("disk.openFiles.action")}
          </Btn>
        </div>
      </div>

      {/* 大目录预期管理：不做中途取消（kill 子进程会留半截结论），文案先到位 */}
      {slow ? (
        <p className="mt-2 text-[10px] text-ink-faint">{t("disk.openFiles.slowHint")}</p>
      ) : null}
      {error ? (
        <p className="mt-2 rounded-lg bg-danger/[0.08] px-2.5 py-1.5 text-[10.5px] text-danger">
          {error}
        </p>
      ) : null}

      {report ? (
        <div className="mt-2.5 border-t border-hairline-soft pt-2.5">
          <p className="text-[10px] text-ink-faint">
            {t("disk.openFiles.summary", {
              n: report.holders.length,
              path: report.path,
              ms: report.scannedMs,
              when: scannedAt ? formatRelative(scannedAt) : "",
            })}
          </p>
          {report.holders.length === 0 ? (
            <p className="mt-1.5 text-[11px] text-ink-dim">
              {t("disk.openFiles.none")}
            </p>
          ) : (
            <ul className="mt-1.5">
              {report.holders.map((h) => (
                <li
                  key={h.pid}
                  className="flex items-baseline justify-between gap-3 rounded-md px-1.5 py-1 transition-colors hover:bg-chip"
                >
                  <span className="min-w-0 truncate text-[11.5px] text-ink">
                    {h.comm}
                    <span className="ml-1.5 font-mono text-[10px] text-ink-faint">
                      {h.pid}
                    </span>
                  </span>
                  <span className="flex shrink-0 items-baseline gap-2 text-[10px] text-ink-faint">
                    <span title={h.sampleFiles.join("\n")}>
                      {t("disk.openFiles.fileCount", { n: h.files })}
                    </span>
                  </span>
                </li>
              ))}
            </ul>
          )}
          {/* Coverage 纪律：盲区说明永远展示——空态 ≠ 没人占用 */}
          {report.coverageNote ? (
            <p className="mt-2 rounded-lg bg-warn/[0.07] px-2.5 py-1.5 text-[10px] leading-relaxed text-warn">
              {report.coverageNote}
            </p>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}
