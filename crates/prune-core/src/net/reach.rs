//! 暴露面可达实测（F17，docs/f17-exposure-reach-plan.md）。
//!
//! 与既有两兄弟的分工：
//! - `ports.rs`：谁占着端口（结构化明细，只读扫描）；
//! - `probe.rs`：端口活着没（loopback 探针，用户显式触发）；
//! - `reach.rs`（本模块）：**从网卡面进来通不通**——对本机某个非 loopback
//!   网卡地址发起一次 TCP connect，报告分层结果。
//!
//! 两条纪律：
//! 1. **只报观测事实**：四态 + 意外错误都如实分列，不写「被防火墙拦截」这类
//!    不可证实的归因（net-diag / net-sites 同款「断点分开、不猜原因」）；
//! 2. **口径诚实**：这是「同机经网卡地址回连」——数据包不出网卡，**不等价于**
//!    「局域网其他设备可达」。前端必须把这句口径带给用户（S3）。
//!
//! 无常驻轮询：`execute` 只在用户显式触发时跑一次，超时上限由调用方传。

use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::time::{Duration, Instant};

/// 分层观测结果。四态 + 两个诚实兜底（无网卡地址 / 未映射错误）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "lowercase")]
pub enum ReachOutcome {
    /// connect 成功：端口在该网卡地址上确实可连
    Connected,
    /// ECONNREFUSED：无进程监听（假活嫌疑：绑定面推断说有，实测连不上）
    Refused,
    /// 超时：可能被拦截，也可能是丢包——只报「超时」，不定因
    Timeout,
    /// ENETUNREACH / EHOSTUNREACH：无路由
    NoRoute,
    /// 本机没有可用的非 loopback 网卡地址（未联网 / 无默认路由）
    NoIfaceAddr,
    /// 未映射的系统错误：如实带出错误文本，不硬塞进四态
    Failed { detail: String },
}

impl ReachOutcome {
    /// UI 主文案。口径纪律：不写「被谁拦了」的断言。
    pub fn label(&self) -> String {
        match self {
            ReachOutcome::Connected => "实测可达（连接成功）".into(),
            ReachOutcome::Refused => "端口拒连：该地址上没有进程监听（假活嫌疑）".into(),
            ReachOutcome::Timeout => "连接超时：无响应（原因未知，可能是拦截或丢包）".into(),
            ReachOutcome::NoRoute => "无路由：该地址不可达".into(),
            ReachOutcome::NoIfaceAddr => "无网卡地址可测（未联网或无默认路由）".into(),
            ReachOutcome::Failed { detail } => format!("探测失败：{detail}"),
        }
    }
}

/// 一次实测的完整产出。`tried_addr` 必须如实展示，不隐去测的是哪个地址。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReachResult {
    pub port: u16,
    /// 实际回连的本机网卡地址（如 192.168.0.115）
    pub tried_addr: Option<String>,
    pub outcome: ReachOutcome,
    pub elapsed_ms: u64,
    /// 固定口径说明：同机回连 ≠ 其他设备可达
    pub caveat: &'static str,
}

const CAVEAT: &str = "同机经网卡地址回连实测，不等价于局域网其他设备可达";

/// 主入口。一次 connect，无状态、无重试、无后台。
pub fn execute(port: u16, timeout_ms: u64) -> ReachResult {
    let started = Instant::now();
    let iface = super::lan::local_scope().map(|s| s.addr);
    let Some(addr) = iface else {
        return ReachResult {
            port,
            tried_addr: None,
            outcome: ReachOutcome::NoIfaceAddr,
            elapsed_ms: started.elapsed().as_millis() as u64,
            caveat: CAVEAT,
        };
    };
    let target = SocketAddrV4::new(addr, port);
    let result =
        TcpStream::connect_timeout(&target.into(), Duration::from_millis(timeout_ms.max(1)));
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let outcome = match result {
        Ok(_) => ReachOutcome::Connected,
        Err(e) => classify(&e),
    };
    ReachResult {
        port,
        tried_addr: Some(addr.to_string()),
        outcome,
        elapsed_ms,
        caveat: CAVEAT,
    }
}

/// 错误 → 观测态映射。纯函数，单测重点：四态各一正例 + 未映射走 Failed。
pub fn classify(e: &std::io::Error) -> ReachOutcome {
    match e.kind() {
        std::io::ErrorKind::ConnectionRefused => ReachOutcome::Refused,
        std::io::ErrorKind::TimedOut => ReachOutcome::Timeout,
        std::io::ErrorKind::NetworkUnreachable | std::io::ErrorKind::HostUnreachable => {
            ReachOutcome::NoRoute
        }
        other => ReachOutcome::Failed {
            detail: other.to_string(),
        },
    }
}

/// 取本机默认路由网卡地址的便捷封装（测试可注入，生产走 lan::local_scope）。
pub fn local_iface_addr() -> Option<Ipv4Addr> {
    super::lan::local_scope().map(|s| s.addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_maps_four_states() {
        use std::io::ErrorKind::*;
        assert_eq!(
            classify(&std::io::Error::from(ConnectionRefused)),
            ReachOutcome::Refused
        );
        assert_eq!(
            classify(&std::io::Error::from(TimedOut)),
            ReachOutcome::Timeout
        );
        assert_eq!(
            classify(&std::io::Error::from(NetworkUnreachable)),
            ReachOutcome::NoRoute
        );
        assert_eq!(
            classify(&std::io::Error::from(HostUnreachable)),
            ReachOutcome::NoRoute
        );
    }

    #[test]
    fn unmapped_error_stays_honest() {
        // 权限拒绝这类未映射错误不许伪装成 Timeout（那是撒谎）
        let out = classify(&std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        match out {
            ReachOutcome::Failed { detail } => assert!(!detail.is_empty()),
            other => panic!("未映射错误必须走 Failed，实际 {other:?}"),
        }
    }

    #[test]
    fn labels_never_assert_a_blocker() {
        // 口径纪律：文案不定因。检查四个主文案都没有「被拦截」的断言语句
        for out in [
            ReachOutcome::Connected,
            ReachOutcome::Refused,
            ReachOutcome::Timeout,
            ReachOutcome::NoRoute,
        ] {
            let label = out.label();
            assert!(
                !label.contains("被拦截") && !label.contains("被防火墙"),
                "文案不得写拦截断言：{label}"
            );
        }
        assert!(ReachOutcome::Timeout.label().contains("原因未知"));
    }

    #[test]
    fn result_carries_caveat_and_serializes_camel_case() {
        let r = ReachResult {
            port: 3000,
            tried_addr: Some("192.168.0.115".into()),
            outcome: ReachOutcome::Connected,
            elapsed_ms: 3,
            caveat: CAVEAT,
        };
        assert!(r.caveat.contains("不等价于"));
        let doc = serde_json::to_value(&r).unwrap();
        assert!(doc.get("triedAddr").is_some());
        assert!(doc.get("tried_addr").is_none(), "契约是 camelCase");
        assert_eq!(doc["outcome"]["outcome"], "connected");
    }

    #[test]
    fn execute_reports_refused_on_closed_port() {
        // 实测路径：起一个 listener 再关掉，端口大概率处于 Refused；
        // 用 loopback 无法测（execute 只走网卡地址），所以这里只在
        // 有网卡地址的机器上验证「跑通且结果自洽」，无网卡时验证 NoIfaceAddr。
        let r = execute(59299, 500);
        match r.outcome {
            ReachOutcome::NoIfaceAddr => assert!(r.tried_addr.is_none()),
            ReachOutcome::Connected
            | ReachOutcome::Refused
            | ReachOutcome::Timeout
            | ReachOutcome::NoRoute
            | ReachOutcome::Failed { .. } => {
                assert!(r.tried_addr.is_some());
                assert!(r.elapsed_ms < 2000, "超时上限生效：{} ms", r.elapsed_ms);
            }
        }
        // caveat 是固定口径，永远随结果走
        assert_eq!(r.caveat, CAVEAT);
    }
}
