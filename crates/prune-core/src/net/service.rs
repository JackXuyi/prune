//! 端口服务类型静态推断（L0，零网络 IO）。
//!
//! 调研见 docs/research/port-service-type-research.md：三层证据按可靠度排序，
//! 命中即停——**这是「疑似」层，L1 探针（`probe.rs`）才能给「确认」**。
//! 推断不出返回 None：UI 不显示「未知服务」这种假确定。
//!
//! 三条纪律（与 net 模块同级）：
//! 1. 系统进程（ControlCenter / rapportd / svchost…）一律不推断——AirPlay 的
//!    5000 端口不是 Web，标出来就是撒谎；
//! 2. 推断顺序：进程名 > 端口表 > cmd/exe 提示 > 项目文件嗅探。越靠前越具体，
//!    端口表对「抢了常见端口的无关进程」会误报，所以排在进程名后面；
//! 3. 只推断 TCP：端口表的语义全是 TCP 服务，UDP 监听（mDNS 5353 等）不套用。
//!
//! Windows 差异：无 cwd（项目归属缺失），由 exe 路径 + cmd 补位；
//! svchost.exe 承载的系统服务不强求归属（SCM 查询需提权），走折叠规则。

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::is_system_process;

/// 端口提供的服务类型。`None` = 推断不出（不是「不是服务」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceKind {
    /// 通用 HTTP 服务（nginx / Caddy / 任意 web 后端）
    Web,
    /// 开发服务器（Vite / Next / uvicorn / Django…）
    DevServer,
    /// 数据库 / 中间件（MySQL / Redis / MQ…）
    Database,
    /// 网络型 MCP server（stdio 型不占端口，探不到——文案必须写清的边界）
    Mcp,
    /// 代理（HTTP / socks / 隧道）
    Proxy,
    /// 调试通道（node --inspect / jdwp）
    Debug,
    /// 认出是服务但归不了类
    Other,
}

/// 端口扫描时随行的进程身份：comm 恒有，exe/cmd 尽力而为。
///
/// Windows 的 iphlpapi 只给 pid，由上层从 MemState 全量进程表借；
/// macOS 的 lsof 自带 comm，exe/cmd 由 MemState 补（有的话推断更准）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcIdentity {
    pub comm: String,
    pub exe: Option<String>,
    pub cmd: Vec<String>,
}

impl ProcIdentity {
    pub fn new(comm: impl Into<String>) -> Self {
        Self {
            comm: comm.into(),
            exe: None,
            cmd: Vec::new(),
        }
    }
}

/// 运行时解释器类进程：它们监听的 loopback 高位端口极可能是 dev server。
///
/// 用途有二：L0 推断的「万金油进程」信号，以及 `is_watched` 的平台无关兜底
/// （Windows 无 cwd → `has_project` 恒空，非标端口的 dev server 会被错误折叠）。
pub fn is_runtime_comm(comm: &str) -> bool {
    let c = comm.to_lowercase();
    // 前缀匹配：python3.11 / python.exe / node-v18 …都是同一族
    [
        "node", "deno", "bun", "python", "java", "dotnet", "ruby", "php", "perl",
    ]
    .iter()
    .any(|p| c.starts_with(p))
}

/// 端口 → 服务类型（L0 静态表）。
///
/// 只放有明确语义的端口；5000/7000 标 DevServer 是**对用户进程**说的——
/// 系统进程（AirPlay）在 `infer` 里已被第 0 条纪律拦掉。
const PORT_SERVICES: &[(u16, ServiceKind)] = &[
    // 前端 dev server（约定俗成的默认端口）
    (3000, ServiceKind::DevServer),
    (3001, ServiceKind::DevServer),
    (3002, ServiceKind::DevServer),
    (3003, ServiceKind::DevServer),
    (4200, ServiceKind::DevServer), // Angular
    (4173, ServiceKind::DevServer), // vite preview
    (4321, ServiceKind::DevServer), // Astro
    (5173, ServiceKind::DevServer), // Vite
    (5174, ServiceKind::DevServer),
    (5175, ServiceKind::DevServer),
    (6006, ServiceKind::DevServer), // Storybook
    (7000, ServiceKind::DevServer),
    (7001, ServiceKind::DevServer),
    (5000, ServiceKind::DevServer), // Flask / Django 默认
    (5001, ServiceKind::DevServer),
    (8000, ServiceKind::DevServer),
    (8001, ServiceKind::DevServer),
    (8002, ServiceKind::DevServer),
    (8888, ServiceKind::Web),
    (9000, ServiceKind::Web),
    (9090, ServiceKind::Web),
    (8080, ServiceKind::Web),
    (8081, ServiceKind::Web),
    (8082, ServiceKind::Web),
    // 数据库 / 中间件
    (3306, ServiceKind::Database),  // MySQL
    (5432, ServiceKind::Database),  // PostgreSQL
    (6379, ServiceKind::Database),  // Redis
    (27017, ServiceKind::Database), // MongoDB
    (27018, ServiceKind::Database),
    (9200, ServiceKind::Database), // Elasticsearch
    (9300, ServiceKind::Database),
    (11211, ServiceKind::Database), // memcached
    (5672, ServiceKind::Database),  // RabbitMQ
    (15672, ServiceKind::Database), // RabbitMQ 管理
    (9092, ServiceKind::Database),  // Kafka
    (2181, ServiceKind::Database),  // ZooKeeper
    // 代理 / 调试 / 其他
    (1080, ServiceKind::Proxy), // socks
    (1087, ServiceKind::Proxy),
    (9229, ServiceKind::Debug), // node inspector
    (9230, ServiceKind::Debug),
];

/// 进程名 → 服务类型。精确匹配（comm / exe 文件名，不含路径）。
const PROC_NAME_KINDS: &[(&str, ServiceKind)] = &[
    ("mysqld", ServiceKind::Database),
    ("mariadbd", ServiceKind::Database),
    ("postgres", ServiceKind::Database),
    ("redis-server", ServiceKind::Database),
    ("mongod", ServiceKind::Database),
    ("mongos", ServiceKind::Database),
    ("memcached", ServiceKind::Database),
    ("elasticsearch", ServiceKind::Database),
    ("rabbitmq-server", ServiceKind::Database),
    ("nginx", ServiceKind::Web),
    ("caddy", ServiceKind::Web),
    ("httpd", ServiceKind::Web),
    ("apache2", ServiceKind::Web),
    ("lighttpd", ServiceKind::Web),
    ("frpc", ServiceKind::Proxy),
    ("frps", ServiceKind::Proxy),
    ("v2ray", ServiceKind::Proxy),
    ("xray", ServiceKind::Proxy),
    ("clash", ServiceKind::Proxy),
    ("mihomo", ServiceKind::Proxy),
    ("sing-box", ServiceKind::Proxy),
    ("mitmproxy", ServiceKind::Proxy),
];

/// cmd / exe 路径的子串提示（小写比对）。顺序敏感：更具体的在前。
const CMD_HINTS: &[(&str, ServiceKind)] = &[
    ("mcp-server", ServiceKind::Mcp),
    ("uvicorn", ServiceKind::DevServer),
    ("flask", ServiceKind::DevServer),
    ("gunicorn", ServiceKind::Web),
    ("http.server", ServiceKind::Web),
    ("vite", ServiceKind::DevServer),
    ("webpack", ServiceKind::DevServer),
    ("next dev", ServiceKind::DevServer),
    ("next start", ServiceKind::DevServer),
    ("--inspect", ServiceKind::Debug),
    ("jdwp", ServiceKind::Debug),
];

/// 项目目录里的强标记文件（glob 前缀比对）。只有「看见它基本就是 dev server」
/// 的文件才进表——`package.json` / `Cargo.toml` 是弱信号（项目未必在起 web 服务）。
const PROJECT_MARKERS: &[&str] = &[
    "vite.config.",
    "next.config.",
    "nuxt.config.",
    "astro.config.",
    "svelte.config.",
    "webpack.config.",
    "manage.py", // Django
];

/// L0 静态推断主入口。命中即停，None = 推断不出。
pub fn infer(
    port: u16,
    proto: super::Proto,
    comm: &str,
    exe: Option<&str>,
    cmd: &[String],
    project: Option<&str>,
) -> Option<ServiceKind> {
    // 纪律 3：端口表全是 TCP 语义，UDP 不套用
    if proto != super::Proto::Tcp {
        return None;
    }
    // 纪律 1：系统进程不推断（AirPlay 的 5000 不是 Web）
    if is_system_process(comm) {
        return None;
    }
    // 纪律 2：进程名（最具体）
    if let Some(kind) = kind_from_comm(comm) {
        return Some(kind);
    }
    // 端口表
    if let Some((_, kind)) = PORT_SERVICES.iter().find(|(p, _)| *p == port) {
        return Some(*kind);
    }
    // cmd / exe 提示
    if let Some(kind) = kind_from_cmd(cmd, exe) {
        return Some(kind);
    }
    // 项目文件嗅探（IO：几次 stat，微秒级）
    if let Some(dir) = project {
        if let Some(kind) = sniff_project_dir(dir) {
            return Some(kind);
        }
    }
    None
}

/// 进程名 / exe 文件名精确匹配。`mysqld.exe` 归一化成 `mysqld` 再比对。
fn kind_from_comm(comm: &str) -> Option<ServiceKind> {
    let lower = comm.to_lowercase();
    PROC_NAME_KINDS
        .iter()
        .find(|(name, _)| lower == *name)
        .map(|(_, kind)| *kind)
}

/// exe 路径的文件名匹配（Windows 的 comm 常是 `node.exe`，类型线索在 exe 上）。
///
/// 路径分隔符同时认 `/` 与 `\`：解析层可能跑在任意平台，而 exe 路径
/// 是「另一平台」的形态（macOS 上解析 Windows 路径是测试常态）。
fn kind_from_exe(exe: Option<&str>) -> Option<ServiceKind> {
    let path = exe?;
    let base = path.rsplit(['/', '\\']).next()?;
    let stem = base.strip_suffix(".exe").unwrap_or(base).to_lowercase();
    if stem.is_empty() {
        return None;
    }
    PROC_NAME_KINDS
        .iter()
        .find(|(name, _)| stem == *name)
        .map(|(_, kind)| *kind)
}

/// cmd 参数拼接后的子串提示 + MCP 专项：argv 里出现 `mcp` 命名的脚本/段
/// （`node …/mcp-server-foo.js`、`python -m mcp`）。
fn kind_from_cmd(cmd: &[String], exe: Option<&str>) -> Option<ServiceKind> {
    // exe 先行：文件名精确表比子串更可信
    if let Some(kind) = kind_from_exe(exe) {
        return Some(kind);
    }
    if cmd.is_empty() {
        return None;
    }
    let joined = cmd.join(" ").to_lowercase();
    // MCP：argv 中出现 mcp 命名的段或文件（避免「mcp」撞上无关路径的误报，
    // 要求它出现在文件名位置——`mcp-server-x.js` / `mcp.js` / 段名恰为 mcp）
    for item in cmd {
        let lower = item.to_lowercase();
        let base = Path::new(&lower)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| lower.clone());
        let is_mcp = base.contains("mcp")
            || lower
                .split('/')
                .chain(lower.split('\\'))
                .any(|seg| seg == "mcp");
        if is_mcp {
            return Some(ServiceKind::Mcp);
        }
    }
    CMD_HINTS
        .iter()
        .find(|(hint, _)| joined.contains(hint))
        .map(|(_, kind)| *kind)
}

/// 项目目录的强标记嗅探（几次 stat）。查不到 / 目录不存在 → None。
pub fn sniff_project_dir(dir: &str) -> Option<ServiceKind> {
    let root = Path::new(dir);
    if !root.is_dir() {
        return None;
    }
    for marker in PROJECT_MARKERS {
        // `vite.config.` 是前缀：vite.config.ts / vite.config.mts 都命中
        let hit = std::fs::read_dir(root).ok()?.any(|entry| {
            entry
                .ok()
                .map(|e| e.file_name().to_string_lossy().starts_with(marker))
                .unwrap_or(false)
        });
        if hit {
            return Some(ServiceKind::DevServer);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn system_processes_never_inferred() {
        // AirPlay 的 5000 端口绝不能标成 Web
        assert_eq!(
            infer(
                5000,
                super::super::Proto::Tcp,
                "ControlCenter",
                None,
                &[],
                None
            ),
            None
        );
        assert_eq!(
            infer(58578, super::super::Proto::Tcp, "rapportd", None, &[], None),
            None
        );
    }

    #[test]
    fn udp_never_inferred() {
        assert_eq!(
            infer(
                5353,
                super::super::Proto::Udp,
                "mDNSResponder",
                None,
                &[],
                None
            ),
            None
        );
    }

    #[test]
    fn process_name_table_wins() {
        assert_eq!(
            infer(40000, super::super::Proto::Tcp, "mysqld", None, &[], None),
            Some(ServiceKind::Database)
        );
        // 进程名优先于端口表：nginx 占着 3000 也不是 dev server
        assert_eq!(
            infer(3000, super::super::Proto::Tcp, "nginx", None, &[], None),
            Some(ServiceKind::Web)
        );
    }

    #[test]
    fn port_table_by_kind() {
        assert_eq!(
            infer(3306, super::super::Proto::Tcp, "java", None, &[], None),
            Some(ServiceKind::Database)
        );
        assert_eq!(
            infer(5173, super::super::Proto::Tcp, "node", None, &[], None),
            Some(ServiceKind::DevServer)
        );
        assert_eq!(
            infer(9229, super::super::Proto::Tcp, "node", None, &[], None),
            Some(ServiceKind::Debug)
        );
        assert_eq!(
            infer(1080, super::super::Proto::Tcp, "gost", None, &[], None),
            Some(ServiceKind::Proxy)
        );
    }

    #[test]
    fn exe_filename_matches() {
        // Windows：comm 是 node.exe，但 exe 路径暴露了真实服务
        assert_eq!(
            infer(
                40000,
                super::super::Proto::Tcp,
                "mysqld.exe",
                Some(r"C:\Program Files\MySQL\bin\mysqld.exe"),
                &[],
                None
            ),
            Some(ServiceKind::Database)
        );
    }

    #[test]
    fn cmd_hints() {
        // node 万金油 + cmd 里的 vite
        assert_eq!(
            infer(
                4455,
                super::super::Proto::Tcp,
                "node",
                Some("/usr/local/bin/node"),
                &s(&["node", "/x/node_modules/.bin/vite", "--port", "4455"]),
                None
            ),
            Some(ServiceKind::DevServer)
        );
        // MCP：argv 里出现 mcp 命名的脚本
        assert_eq!(
            infer(
                4460,
                super::super::Proto::Tcp,
                "node",
                Some("/usr/local/bin/node"),
                &s(&["node", "/x/servers/mcp-server-fs/index.js"]),
                None
            ),
            Some(ServiceKind::Mcp)
        );
        // node inspector
        assert_eq!(
            infer(
                4461,
                super::super::Proto::Tcp,
                "node",
                None,
                &s(&["node", "--inspect", "app.js"]),
                None
            ),
            Some(ServiceKind::Debug)
        );
    }

    #[test]
    fn mcp_false_positive_resisted() {
        // 「mcp」出现在目录段但不是文件名 / 不是 mcp 段：不误报
        // （/my-mcp-project/ 里的 vite 脚本 → vite 提示命中，不是 Mcp）
        assert_eq!(
            infer(
                4470,
                super::super::Proto::Tcp,
                "node",
                None,
                &s(&["node", "/Users/x/my-mcp-project/vite.js"]),
                None
            ),
            Some(ServiceKind::DevServer)
        );
    }

    #[test]
    fn project_marker_sniff() {
        // 临时目录造一个 vite.config.ts
        let dir = std::env::temp_dir().join(format!("prune-svc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("vite.config.ts"), "export default {}").unwrap();
        assert_eq!(
            sniff_project_dir(dir.to_str().unwrap()),
            Some(ServiceKind::DevServer)
        );
        // 空项目目录 → None
        let empty = std::env::temp_dir().join(format!("prune-svc-empty-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(sniff_project_dir(empty.to_str().unwrap()), None);
        // 不存在的目录 → None
        assert_eq!(sniff_project_dir("/nonexistent-prune-svc"), None);
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&empty).ok();
    }

    #[test]
    fn project_sniff_wins_over_nothing() {
        let dir = std::env::temp_dir().join(format!("prune-svc2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("next.config.mjs"), "export default {}").unwrap();
        // 非标端口 + 万金油进程 + 无 cmd：项目嗅探兜底
        assert_eq!(
            infer(
                8123,
                super::super::Proto::Tcp,
                "node",
                None,
                &[],
                Some(dir.to_str().unwrap())
            ),
            Some(ServiceKind::DevServer)
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn runtime_comm_prefix_match() {
        assert!(is_runtime_comm("node"));
        assert!(is_runtime_comm("python3.11"));
        assert!(is_runtime_comm("python.exe"));
        assert!(is_runtime_comm("java"));
        assert!(is_runtime_comm("deno"));
        // 前缀匹配的代价：nodemanager 这类以 node 开头的进程也会命中——
        // 兜底规则只影响「是否进关注层」，多看一条无害；推断层不依赖它
        assert!(is_runtime_comm("nodemanager"));
        assert!(!is_runtime_comm("Cursor Helper"));
        assert!(!is_runtime_comm("ondo"));
    }
}
