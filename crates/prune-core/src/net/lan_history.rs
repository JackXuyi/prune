//! 局域网设备时间线（M19，docs/m19-lan-device-times-spec.md）。
//!
//! 定位：给 [`super::lan`] 的每轮扫描加**跨轮次记忆**——设备首次发现时间
//! （`first_seen`）、最后活跃时间（`last_seen`）、新设备标识（`is_new`）。
//!
//! 口径纪律（spec 决策 1–5，改任何一条必须先改 spec）：
//! 1. 身份 key：**稳定 MAC 优先**（非随机化），随机化 / 无 MAC 回退 `ip:<IP>@<cidr>`
//!    ——DHCP 换 IP 不算新设备，这是「新设备标识」可信度的根基；
//!    M18 文案里「以 IP 为准」是当时无记忆状态下的诚实表述，M19 已修订口径并同步 UI 文案。
//! 2. `is_new` 以 `snapshot.at` 为基准，不读墙钟——缓存快照重启后不失真。
//! 3. 首扫基线：history 落盘前为空 ⇒ 本轮全员不标新（否则冷启动满屏「新」）。
//! 4. `#[serde(default)]`：旧版快照缓存无新字段也不炸（升级不丢缓存）。
//!
//! 模块分层：`device_key` / `apply_history` / `gc_history` 全是纯函数（可单测），
//! IO 只有 `load_history` / `store_history` 两个薄壳 + `record` 一步到位壳。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::lan::{LanDevice, LanSnapshot};

/// 新设备窗口（spec 已确认：const，不做设置项）
pub const NEW_WINDOW_SECS: i64 = 600;
/// 不活跃回收线：30 天没见过即从历史剔除
pub const RETAIN_SECS: i64 = 30 * 86_400;
/// 历史总量上限（超出按 last_seen 最旧淘汰）
pub const HISTORY_CAP: usize = 1024;

// ---------------------------------------------------------------- 数据结构

/// 跨轮次设备历史（`lan-history.json` 的 serde 形态）。
///
/// 只存身份 + 时间 + 定位信息，**不落 services / vendor / hostname**
/// （隐私最小面；这些每轮扫描都能重取）。不进任何上报，不出本机。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LanHistory {
    pub version: u32,
    pub devices: BTreeMap<String, LanHistoryEntry>,
}

/// 单台设备的跨轮次记录。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LanHistoryEntry {
    pub first_seen: i64,
    pub last_seen: i64,
    /// mac-key 设备 DHCP 换地址后的最新 IP（ip-key 设备 = key 里的 IP 本身）
    pub ip: String,
    /// ip-key 的绑定网段（换网即换锚）；mac-key 也带上便于诊断
    pub cidr: Option<String>,
}

// ---------------------------------------------------------------- 纯函数

/// 设备身份 key（spec 决策 1 的唯一落点）。
///
/// - MAC 可用且非随机化 → `mac:<MAC>`（DHCP 换 IP 不算新设备；跨网段仍可认）
/// - 否则 → `ip:<IP>@<cidr>`（绑定网段：换网后的同 IP 是另一台设备）
pub fn device_key(d: &LanDevice, cidr: Option<&str>) -> String {
    match (&d.mac, d.randomized_mac) {
        (Some(mac), false) => format!("mac:{mac}"),
        _ => format!("ip:{}@{}", d.ip, cidr.unwrap_or("")),
    }
}

/// 回填 `first_seen` / `last_seen` / `is_new` 并更新历史。
///
/// 返回 `baseline_existed`（本轮是否在已有历史上跑——`false` = 首扫基线）。
/// 快照不可用（`at == 0` 或 `cidr == None`）时调用方不得调用本函数
/// （[`record`] 已挡）。
///
/// 不变式（spec §4，单测锁定）：
/// 1. 命中 entry 时 `first_seen` 永不被覆盖（时间线只增不改）；
/// 2. 首扫基线时任何设备 `is_new` 都是 false；
/// 3. `is_new` 只依赖 `snapshot.at` 与 `first_seen`；
/// 4. 历史损坏 = 空 history = 本轮当基线，不阻塞扫描主流程（load_history 已兜）。
pub fn apply_history(snap: &mut LanSnapshot, history: &mut LanHistory) -> bool {
    let baseline_existed = !history.devices.is_empty();
    let at = snap.at;
    let cidr = snap.cidr.clone();

    for device in &mut snap.devices {
        let key = device_key(device, cidr.as_deref());
        match history.devices.get_mut(&key) {
            Some(e) => {
                // 不变式 1：first_seen 只在首次插入时定下
                device.first_seen = e.first_seen;
                device.last_seen = at;
                e.last_seen = at;
                e.ip = device.ip.clone();
                e.cidr = cidr.clone();
            }
            None => {
                device.first_seen = at;
                device.last_seen = at;
                history.devices.insert(
                    key,
                    LanHistoryEntry {
                        first_seen: at,
                        last_seen: at,
                        ip: device.ip.clone(),
                        cidr: cidr.clone(),
                    },
                );
            }
        }
    }

    for device in &mut snap.devices {
        device.is_new = baseline_existed && (at - device.first_seen) <= NEW_WINDOW_SECS;
    }

    gc_history(history, at);

    if !baseline_existed {
        snap.notes.push(
            "本轮为首次扫描，已建立设备基线；后续扫描将标记 10 分钟内新接入的设备。".to_string(),
        );
    }
    baseline_existed
}

/// 回收 + 淘汰：30 天没见过剔除；超 cap 按 last_seen 最旧淘汰。
pub fn gc_history(history: &mut LanHistory, now: i64) {
    history
        .devices
        .retain(|_, e| e.last_seen >= now - RETAIN_SECS);
    if history.devices.len() > HISTORY_CAP {
        // last_seen 升序排，最旧的先淘汰（BTreeMap 没有 drain，借 take 中转）
        let mut entries: Vec<(String, LanHistoryEntry)> =
            std::mem::take(&mut history.devices).into_iter().collect();
        entries.sort_by_key(|(_, e)| e.last_seen);
        let drop = entries.len() - HISTORY_CAP;
        history.devices.extend(entries.into_iter().skip(drop));
    }
}

// ---------------------------------------------------------------- IO 薄壳

/// 读历史层。文件缺失 / 解析失败（写一半 / 手改坏 / 版本不认识）一律当空历史——
/// 本轮当首扫基线，绝不阻塞扫描主流程（不变式 4）。
pub fn load_history() -> LanHistory {
    let parsed = crate::paths::lan_history_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<LanHistory>(&t).ok());
    parsed.unwrap_or(LanHistory {
        version: 1,
        devices: BTreeMap::new(),
    })
}

/// 原子写历史层（tmp + rename，与 `persist_cache` 同套路）。写失败只影响
/// 下一轮时间线，不影响本轮扫描返回。
pub fn store_history(history: &LanHistory) {
    let Some(path) = crate::paths::lan_history_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(history) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// 一步到位：GUI（`lan_scan`）与 CLI（`cmd_lan`）都在拿到快照后调它。
///
/// 不可用快照（`unavailable` / 读不到网段：`at == 0` 或 `cidr == None`）直接跳过
/// ——空设备列表也值得跳过写入：那会白建立一条基线并误标后续真扫描为「全员新」。
pub fn record(snap: &mut LanSnapshot) {
    if snap.at == 0 || snap.cidr.is_none() {
        return;
    }
    let mut history = load_history();
    apply_history(snap, &mut history);
    store_history(&history);
}

// ---------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::lan::{DeviceKind, NeighborState};

    /// 合成一台最小设备（时间线三字段由 apply_history 回填，此处给 0）
    fn device(ip: &str, mac: Option<&str>, randomized: bool) -> LanDevice {
        LanDevice {
            ip: ip.to_string(),
            mac: mac.map(str::to_string),
            vendor: None,
            hostname: None,
            kind: DeviceKind::Phone,
            state: NeighborState::Reachable,
            online: true,
            services: Vec::new(),
            inbound: Vec::new(),
            is_gateway: false,
            is_self: false,
            randomized_mac: randomized,
            first_seen: 0,
            last_seen: 0,
            is_new: false,
        }
    }

    fn snapshot(at: i64, cidr: &str, devices: Vec<LanDevice>) -> LanSnapshot {
        LanSnapshot {
            at,
            iface: Some("en0".into()),
            cidr: Some(cidr.into()),
            scanned: 0,
            total: 0,
            devices,
            exposed_ports: Vec::new(),
            notes: Vec::new(),
            scan_ms: 0,
        }
    }

    fn entry(first_seen: i64, last_seen: i64, ip: &str) -> LanHistoryEntry {
        LanHistoryEntry {
            first_seen,
            last_seen,
            ip: ip.into(),
            cidr: Some("192.168.0.0/24".into()),
        }
    }

    const CIDR: &str = "192.168.0.0/24";
    const STABLE: &str = "BC:24:11:4C:D8:39";
    const RANDOM: &str = "F6:74:4D:E9:41:5E";

    #[test]
    fn key_mac_stable() {
        // 非 随机化 MAC → mac key，不带 cidr（跨网段仍可认）
        assert_eq!(
            device_key(&device("192.168.0.44", Some(STABLE), false), Some(CIDR)),
            "mac:BC:24:11:4C:D8:39"
        );
        // 随机化 MAC → ip key，绑定网段
        assert_eq!(
            device_key(&device("192.168.0.44", Some(RANDOM), true), Some(CIDR)),
            "ip:192.168.0.44@192.168.0.0/24"
        );
        // 无 MAC → ip key
        assert_eq!(
            device_key(&device("192.168.0.44", None, false), Some(CIDR)),
            "ip:192.168.0.44@192.168.0.0/24"
        );
    }

    #[test]
    fn first_scan_baseline() {
        let mut snap = snapshot(
            1_000,
            CIDR,
            vec![device("192.168.0.44", Some(STABLE), false)],
        );
        let mut history = LanHistory::default();
        let baseline = apply_history(&mut snap, &mut history);
        assert!(!baseline, "空 history = 首扫基线");
        let d = &snap.devices[0];
        assert_eq!(d.first_seen, 1_000);
        assert_eq!(d.last_seen, 1_000);
        assert!(!d.is_new, "不变式 2：基线轮全员不标新");
        assert!(snap.notes.iter().any(|n| n.contains("首次扫描")));
    }

    #[test]
    fn hit_preserves_first_seen() {
        let mut snap = snapshot(
            2_000,
            CIDR,
            vec![device("192.168.0.44", Some(STABLE), false)],
        );
        let mut history = LanHistory::default();
        history
            .devices
            .insert(format!("mac:{STABLE}"), entry(1_000, 1_000, "192.168.0.44"));
        let baseline = apply_history(&mut snap, &mut history);
        assert!(baseline);
        let d = &snap.devices[0];
        // 不变式 1：first_seen 永不被覆盖
        assert_eq!(d.first_seen, 1_000);
        assert_eq!(d.last_seen, 2_000);
    }

    #[test]
    fn new_within_window() {
        // 上一轮 5 分钟前建立基线，本轮命中 → 仍是「新」
        let mut snap = snapshot(
            1_300,
            CIDR,
            vec![device("192.168.0.44", Some(STABLE), false)],
        );
        let mut history = LanHistory::default();
        history
            .devices
            .insert(format!("mac:{STABLE}"), entry(1_000, 1_000, "192.168.0.44"));
        apply_history(&mut snap, &mut history);
        assert!(snap.devices[0].is_new);
    }

    #[test]
    fn not_new_beyond_window() {
        let mut snap = snapshot(
            1_601,
            CIDR,
            vec![device("192.168.0.44", Some(STABLE), false)],
        );
        let mut history = LanHistory::default();
        history
            .devices
            .insert(format!("mac:{STABLE}"), entry(1_000, 1_000, "192.168.0.44"));
        apply_history(&mut snap, &mut history);
        assert!(!snap.devices[0].is_new);
    }

    #[test]
    fn mac_key_survives_ip_change() {
        // 同 MAC 换 IP（DHCP 续租）：仍命中同一 entry，ip 更新
        let mut snap = snapshot(
            2_000,
            CIDR,
            vec![device("192.168.0.99", Some(STABLE), false)],
        );
        let mut history = LanHistory::default();
        history
            .devices
            .insert(format!("mac:{STABLE}"), entry(1_000, 1_000, "192.168.0.44"));
        apply_history(&mut snap, &mut history);
        assert_eq!(snap.devices[0].first_seen, 1_000, "换 IP 不算新设备");
        assert_eq!(
            history.devices.get(&format!("mac:{STABLE}")).unwrap().ip,
            "192.168.0.99"
        );
    }

    #[test]
    fn ip_key_binds_cidr() {
        // 同 IP 换网段：不命中，视为新锚（baseline 对整个 history 而言仍存在）
        let mut snap = snapshot(
            2_000,
            "10.0.0.0/24",
            vec![device("192.168.0.44", None, false)],
        );
        let mut history = LanHistory::default();
        history.devices.insert(
            "ip:192.168.0.44@192.168.0.0/24".into(),
            entry(1_000, 1_000, "192.168.0.44"),
        );
        let baseline = apply_history(&mut snap, &mut history);
        assert!(baseline);
        // 新锚：first_seen = 本轮 at，且在窗口内 → 标新
        assert_eq!(snap.devices[0].first_seen, 2_000);
        assert!(snap.devices[0].is_new);
        // 旧网段的 entry 不被污染，新锚独立成条
        assert!(history
            .devices
            .contains_key("ip:192.168.0.44@192.168.0.0/24"));
        assert!(history.devices.contains_key("ip:192.168.0.44@10.0.0.0/24"));
    }

    #[test]
    fn gc_retains_and_drops() {
        let mut history = LanHistory::default();
        let now = 30 * 86_400 + 1_000;
        history
            .devices
            .insert("a".into(), entry(now - 100, now - 100, "1.1.1.1"));
        history.devices.insert(
            "b".into(),
            entry(now - 31 * 86_400, now - 31 * 86_400, "1.1.1.2"),
        );
        gc_history(&mut history, now);
        assert!(history.devices.contains_key("a"));
        assert!(!history.devices.contains_key("b"), "30 天没见过 → 剔除");
    }

    #[test]
    fn cap_evicts_oldest() {
        let mut history = LanHistory::default();
        let now = 1_000_000;
        for i in 0..(HISTORY_CAP + 10) as i64 {
            history
                .devices
                .insert(format!("k{i}"), entry(i, i, &format!("10.0.0.{i}")));
        }
        gc_history(&mut history, now);
        assert_eq!(history.devices.len(), HISTORY_CAP);
        // 被淘汰的是 last_seen 最旧的 k0..k9
        for i in 0..10i64 {
            assert!(!history.devices.contains_key(&format!("k{i}")));
        }
        assert!(history
            .devices
            .contains_key(&format!("k{}", HISTORY_CAP as i64 + 9)));
    }

    #[test]
    fn serde_default_legacy() {
        // 旧版快照缓存没有时间线三字段：反序列化不炸，值兜 0 / 0 / false
        let legacy = r#"{
            "ip": "192.168.0.44",
            "mac": "BC:24:11:4C:D8:39",
            "vendor": null,
            "hostname": null,
            "kind": "phone",
            "state": "reachable",
            "online": true,
            "services": [],
            "inbound": [],
            "isGateway": false,
            "isSelf": false,
            "randomizedMac": false
        }"#;
        let d: LanDevice = serde_json::from_str(legacy).unwrap();
        assert_eq!(d.first_seen, 0);
        assert_eq!(d.last_seen, 0);
        assert!(!d.is_new);
    }

    #[test]
    fn record_skips_unavailable_snapshot() {
        // 不可用快照（at == 0）不建立基线——否则下一轮真扫描全员误标「新」
        let mut snap = LanSnapshot::unavailable("读不到默认网卡的网段");
        record(&mut snap);
        assert!(snap.devices.is_empty());
        // record 内部 load/store 走真实路径，这里只验证它没把不可用快照写穿
    }
}
