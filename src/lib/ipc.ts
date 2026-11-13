/** 与 Rust 侧 IPC 命令的薄封装（契约见 docs/m2-memory-plan.md §6 / docs/m1-disk-plan.md §8）。 */
import { invoke } from "@tauri-apps/api/core";

import type {
  ArchiveEntry,
  ArchivePlan,
  DiskReport,
  ReclaimChild,
  ReclaimItem,
  ReclaimPreview,
  ReclaimReport,
  RestorePreview,
  RestoreReport,
  VolumeInfo,
} from "../types/disk";
import type { LanSnapshot } from "../types/lan";
import type { AppDisk, BundleMap, BundleQuery } from "../types/appdisk";
import type { DailyBucket, HistoryBucket, IconMap, IconQuery } from "../types/appHistory";
import type { LeakCase, MemSnapshot, Point, ProcMem } from "../types/mem";
import type {
  DayTraffic,
  NetDiag,
  NetDiagCapability,
  NetSpeedSample,
  NetTotalPoint,
  NetTotalSample,
  PortSnapshot,
  ProbeOutcome,
  SpeedResult,
} from "../types/net";

/** 浏览器里跑 vite 时没有任何 Tauri 注入，需要显式兜底而不是静默失败 */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * 「发后不管」的显式出口：捕获 rejection 并留一条 console.warn，而不是让
 * unhandled rejection 在 WKWebView 里静默消失（用户表现为「点了没反应」）。
 * 只用于无返回值、失败也不影响主流程的调用（岛显隐、开主窗口等）；
 * 有结果反馈的调用仍应 await + catch 落到 store error。
 */
export function fireAndForget(task: Promise<unknown>, label: string): void {
  // dev 日志用英文：它不是 UI 文案（真正的错误提示走 store → t()）
  task.catch((e) => console.warn(`[prune] ${label} failed:`, e));
}

export function memSnapshot(): Promise<MemSnapshot | null> {
  return invoke<MemSnapshot | null>("mem_snapshot");
}

export function memProcesses(): Promise<ProcMem[]> {
  return invoke<ProcMem[]>("mem_processes");
}

export function memHistory(pid: number, minutes: number): Promise<Point[]> {
  return invoke<Point[]>("mem_history", { pid, minutes });
}

/** 悬案卷宗快照（M22）：侦查中的在前，已归档在后。案件簿在内存，重启即清零 */
export function leakCases(): Promise<LeakCase[]> {
  return invoke<LeakCase[]>("leak_cases");
}

export function memRefresh(): Promise<void> {
  return invoke<void>("mem_refresh");
}

/** 打开主窗口；传 pid 时主窗口会自动选中该进程，传 tab 时直接切到对应 tab，
 *  传 settings=true 时直接弹出设置弹层（面板齿轮走这条）。
 *  v3.1 IA 收敛后 tab 集 = 6 页；旧 id（apps/overview/port）由主窗口 mapExternalTab 兼容映射。 */
export function openMain(
  pid?: number,
  tab?: "stats" | "resources" | "memory" | "disk" | "ports" | "net",
  settings?: boolean,
): Promise<void> {
  return invoke<void>("open_main", { pid: pid ?? null, tab: tab ?? null, settings: settings ?? null });
}

/** 结束进程：默认 SIGTERM（可优雅退出），force=true 时 SIGKILL */
export function killProcess(pid: number, force: boolean): Promise<void> {
  return invoke<void>("kill_process", { pid, force });
}

/** 在 Finder 中打开目录；传文件路径时定位并选中它 */
export function revealPath(path: string): Promise<void> {
  return invoke<void>("reveal_path", { path });
}

// ---------------------------------------------------------------- 磁盘（M1）

export function diskReport(): Promise<DiskReport | null> {
  return invoke<DiskReport | null>("disk_report");
}

/** 触发一轮扫描（单飞：已有扫描在跑时直接返回） */
export function diskScan(): Promise<void> {
  return invoke<void>("disk_scan");
}

/**
 * M9 统计 tab：home 所在卷容量（statvfs 直读，不依赖扫描）。
 * null = statvfs 失败（罕见），UI 显示「容量不可读」而不是 0。
 */
export function diskVolume(): Promise<VolumeInfo | null> {
  return invoke<VolumeInfo | null>("disk_volume");
}

/** dry-run：只生成清单，不动任何文件 */
export function reclaimPreview(keys: string[]): Promise<ReclaimPreview> {
  return invoke<ReclaimPreview>("reclaim_preview", { keys });
}

/** 一级子项体积明细（du 较慢，独立按需加载；预览弹层先出、内容后到） */
export function reclaimChildren(path: string): Promise<ReclaimChild[]> {
  return invoke<ReclaimChild[]>("reclaim_children", { path });
}

/** 执行清理（走废纸篓）；成功后 Rust 侧会自动重扫并发 disk://done */
export function reclaimExecute(items: ReclaimItem[], confirmed: boolean): Promise<ReclaimReport> {
  return invoke<ReclaimReport>("reclaim_execute", { items, confirmed });
}

// ---------------------------------------------------------------- 剪除成绩单（M21）

/** 按分组聚合的一行 Top N（label 来自清理分组，如「npm 缓存」「node_modules」） */
export interface CutTop {
  label: string;
  bytes: number;
  count: number;
}

/** 剪除成绩单：累计 / 次数 / 首次时间 / Top 5。全 0 + 空 Top = 还没剪过 */
export interface CutReport {
  totalBytes: number;
  totalItems: number;
  firstTs: number | null;
  top: CutTop[];
}

/** 剪除成绩单（M21）：数据来自用户亲手执行过的清理，append-only 落在本地库 */
export function cutReport(): Promise<CutReport> {
  return invoke<CutReport>("cut_report");
}

// ---------------------------------------------------------------- 端口（M7）

/** 最新一轮端口快照；null = 还没扫过（**不是「没有端口」**） */
export function portSnapshot(): Promise<PortSnapshot | null> {
  return invoke<PortSnapshot | null>("port_snapshot");
}

/** 手动刷新（用户点了就该扫，绕过 30 s 定时；单飞闸门仍在） */
export function portRefresh(): Promise<void> {
  return invoke<void>("port_refresh");
}

/** 可见性门控：false 时定时器完全不采样，常驻开销 0 */
export function portSetActive(active: boolean): Promise<void> {
  return invoke<void>("port_set_active", { active });
}

/**
 * L1 服务探针：与端口做一次协议握手（Web HEAD / MCP initialize / banner）。
 * 用户显式触发——它发包；Rust 侧按 `(port, pid, startTime)` 缓存，重复点击零开销。
 */
export function portProbe(
  port: number,
  pid: number | null,
  startTime: number,
): Promise<ProbeOutcome> {
  return invoke<ProbeOutcome>("port_probe", { port, pid, startTime });
}

// ---------------------------------------------------------------- 应用磁盘（M8）

/**
 * 测量一个应用的磁盘三层（本体 / 缓存 / 数据）。
 *
 * 这是**按需**调用：Rust 侧有 30 min TTL 缓存，`force` 才会重测。
 * 实测耗时 20 ms（Docker）到 4 s（Chrome profile 撞预算上限），
 * 所以调用方必须有「测量中」占位，不能假定它即时返回。
 */
export function appDisk(
  family: string,
  exe: string | null,
  force = false,
): Promise<AppDisk> {
  return invoke<AppDisk>("app_disk", { family, exe, force });
}

/**
 * M10：批量测一批应用的**本体**体积（卡片首屏用）。
 * 8 路并行 + 4 s/应用预算 + 30 min 缓存；超预算条目 `partial = true`。
 */
export function appBundles(items: BundleQuery[], force = false): Promise<BundleMap> {
  return invoke<BundleMap>("app_bundles", { items, force });
}

/**
 * D1：批量取应用图标（一次 IPC）。返回 `family -> data:image/png;base64,...`；
 * 拿不到的家族**不在 map 里**（前端落字母徽标，不显示占位灰图）。
 */
export function appIcons(items: IconQuery[]): Promise<IconMap> {
  return invoke<IconMap>("app_icons", { items });
}

/**
 * D3：某应用近 N 小时的历史聚合。
 * 只返回**有记录的小时**，缺失由前端画灰柱（不补 0）。
 */
export function appHistory(family: string, hours: number): Promise<HistoryBucket[]> {
  return invoke<HistoryBucket[]>("app_history", { family, hours });
}

/**
 * M25：某应用近 N 天的天粒度聚合（后端 SQL 聚合，30 天只回 30 行）。
 * 只返回**有记录的天**，缺失由前端画灰柱（不补 0）。
 */
export function appHistoryDaily(family: string, days: number): Promise<DailyBucket[]> {
  return invoke<DailyBucket[]>("app_history_daily", { family, days });
}

/**
 * D7：把文本写入系统剪贴板（Rust 侧 `pbcopy` / `Set-Clipboard`）。
 * 返回写入字符数；不可用平台返回 Err（前端再走 navigator 兜底）。
 */
export function clipboardWrite(text: string): Promise<number> {
  return invoke<number>("clipboard_write", { text });
}

// ---------------------------------------------------------------- 网速（M10）

/** 最新一轮网速样本；null = 还没采过（**不是「没有流量」**） */
export function netSpeedSnapshot(): Promise<NetSpeedSample | null> {
  return invoke<NetSpeedSample | null>("net_speed_snapshot");
}

/** 可见性门控：false 时 1 s 采样循环连 nettop 都不 spawn，常驻开销 0 */
export function netSpeedSetActive(active: boolean): Promise<void> {
  return invoke<void>("net_speed_set_active", { active });
}

// ---------------------------------------------------------------- 网络总览（M14）

/** 最新一拍系统口径网速；null = 还没采过（**不是「没有流量」**） */
export function netTotalSnapshot(): Promise<NetTotalSample | null> {
  return invoke<NetTotalSample | null>("net_total_snapshot");
}

/** 走势窗口（60 / 300 / 900 秒），后端已降采样到 ≤ 120 点 */
export function netTotalHistory(seconds: number): Promise<NetTotalPoint[]> {
  return invoke<NetTotalPoint[]>("net_total_history", { seconds });
}

/** 近 N 天每日流量（升序，只含有记录的日子；缺失日不返回） */
export function netTrafficRecent(days: number): Promise<DayTraffic[]> {
  return invoke<DayTraffic[]>("net_traffic_recent", { days });
}

/**
 * 只改采样节拍（1 s ↔ 2 s），不清基线。
 * 与 `netSpeedSetActive` 不同：那个是「开不开 nettop」的开关
 */
export function netTotalSetActive(active: boolean): Promise<void> {
  return invoke<void>("net_total_set_active", { active });
}

// ---------------------------------------------------------------- 自动升级

/** 检查结果负载（Rust 侧 updater::pending_payload / check-result 事件的形状） */
export type UpdateStatus =
  | { status: "available"; version: string; currentVersion: string; notes: string | null; date: string | null }
  | { status: "up-to-date" }
  | { status: "error"; message: string }
  | { status: "none" };

/** 手动检查更新（托盘 / 面板共用同一 Rust 逻辑，结果也会经事件广播） */
export function updateCheck(): Promise<UpdateStatus> {
  return invoke<UpdateStatus>("update_check");
}

/** 查询本次会话已发现的待安装更新（面板重开后的状态兜底） */
export function updatePending(): Promise<UpdateStatus> {
  return invoke<UpdateStatus>("update_pending");
}

/** 下载并安装待更新，完成后应用自动重启 */
export function updateInstall(): Promise<void> {
  return invoke<void>("update_install");
}

// ---------------------------------------------------------------- 网络诊断（net-diag-plan）

/** 跑一轮分层诊断（~5 s，手动触发；结果同时存后端供报告导出） */
export function netDiagRun(): Promise<NetDiag> {
  return invoke<NetDiag>("net_diag_run");
}

/** 测速 + bufferbloat（macOS networkQuality，~30 s）。Windows 上会返回 Err（capability 门控） */
export function netDiagSpeedRun(): Promise<SpeedResult> {
  return invoke<SpeedResult>("net_diag_speed_run");
}

/** 诊断能力：前端据 speedTest 决定是否渲染测速卡（不硬编码平台） */
export function netDiagCapability(): Promise<NetDiagCapability> {
  return invoke<NetDiagCapability>("net_diag_capability");
}

/** 生成 Markdown 诊断报告并写入系统剪贴板，返回字符数（Rust 侧原生写，绕开 WebView 剪贴板限制） */
export function netDiagCopyReport(): Promise<number> {
  return invoke<number>("net_diag_copy_report");
}

// ---- M18：局域网设备（docs/m18-lan-devices-spec.md §10）----

/** 读最近一轮局域网快照缓存，不扫描（进分段时调，不会触发权限弹窗）。null = 还没扫过 */
export function lanSnapshot(): Promise<LanSnapshot | null> {
  return invoke<LanSnapshot | null>("lan_snapshot");
}

/** 全段扫描一轮（探测 ~5s + mDNS 30s 窗口），进度走 lan://progress 事件 */
export function lanScan(): Promise<LanSnapshot> {
  return invoke<LanSnapshot>("lan_scan");
}

// ---- M30 项目休眠归档（docs/f14-m30-archive-plan.md）----

/** 归档账本（按归档时间倒序） */
export function archiveList(): Promise<ArchiveEntry[]> {
  return invoke<ArchiveEntry[]>("archive_list");
}

/** dry-run：统计依赖目录体积与目标归档路径，不写任何文件 */
export function archivePlan(projectRoot: string): Promise<ArchivePlan> {
  return invoke<ArchivePlan>("archive_plan", { projectRoot });
}

/** 执行归档（必须 confirmed）：校验通过后才删原件 */
export function archiveCreate(projectRoot: string, confirmed: boolean): Promise<ArchiveEntry> {
  return invoke<ArchiveEntry>("archive_create", { projectRoot, confirmed });
}

/** dry-run：恢复目标 / 体积 / 冲突标记 */
export function archiveRestorePreview(id: string): Promise<RestorePreview> {
  return invoke<RestorePreview>("archive_restore_preview", { id });
}

/** 执行恢复（必须 confirmed）：overwrite 用于目标已存在的二次确认 */
export function archiveRestore(id: string, confirmed: boolean, overwrite: boolean): Promise<RestoreReport> {
  return invoke<RestoreReport>("archive_restore", { id, confirmed, overwrite });
}

/** 取消当前归档/恢复：返回「是否真的有一个任务在跑」 */
export function archiveCancel(): Promise<boolean> {
  return invoke<boolean>("archive_cancel");
}

/** 删除归档（必须 confirmed）：归档文件走废纸篓 + 移除账本条目 */
export function archiveDelete(id: string, confirmed: boolean): Promise<void> {
  return invoke<void>("archive_delete", { id, confirmed });
}
