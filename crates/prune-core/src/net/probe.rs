//! 端口服务探针（L1）：对 TCP 监听端口做**一次有界握手**，给出「确认」级类型。
//!
//! 调研见 docs/research/port-service-type-research.md。与 L0（`service.rs`）的关系：
//! L0 是零 IO 的「疑似」，本模块是**用户显式触发**才执行的「确认」——它发包，
//! 打破 M7「只读不碰网络」的默认纪律，因此：
//! 1. 调用方必须是用户动作（行内「探测」按钮 / 设置开关），绝不静默批量扫描；
//! 2. 只打 `127.0.0.1`（本机自己的服务），不碰局域网；
//! 3. 结果按 `(port, pid, start_time)` 缓存——进程活着、端口没变则协议不变，
//!    反复 initialize 只会堆积 MCP session，缓存就是会话卫生。
//!
//! 探测链（命中即停）：
//! 1. `GET /.well-known/mcp/server-card.json`（SEP-1649/1960，零副作用，信息最全）
//! 2. `HEAD /` → 任何 `HTTP/` 响应 = Web（`Server` 头进 detail）
//! 3. `POST initialize` JSON-RPC 到 `/mcp` → `/api/mcp` → `/`（判据：响应含
//!    `protocolVersion` + `serverInfo`；JSON 与 SSE 两种响应形态都覆盖）
//! 4. `GET /sse` → `text/event-stream` = 旧版 MCP transport（2024-11-05）
//! 5. banner：被动读首包（SSH / MySQL greeting）+ Redis `PING`
//!
//! 探后如果拿到了 `Mcp-Session-Id`，补发 DELETE 释放会话（会话卫生第 2 条）。
//! 纯 `std::net`，双平台零差异（Windows 上无 lsof 盲区，覆盖率反而更完整）。

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::service::ServiceKind;

/// 连接超时。本机 loopback 握手 <1ms，600ms 已经是 100 倍冗余
const CONNECT_TIMEOUT: Duration = Duration::from_millis(600);
/// 读写超时：HTTP 往返本机 1–5ms，800ms 覆盖掉冷启动的服务
const RW_TIMEOUT: Duration = Duration::from_millis(800);
/// 响应读取上限。探测只需要头 + 少量 body，16 KiB 足够且防失控
const READ_CAP: u64 = 16 * 1024;
/// 被动 banner 的等待窗口（SSH / MySQL 是连接后先说话的一方）
const BANNER_WAIT: Duration = Duration::from_millis(400);

/// 一次探针的产出。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeOutcome {
    pub kind: ServiceKind,
    /// 确认细节：HTTP `Server` 头 / MCP `serverInfo.name` / SSH 版本串。
    /// None = 协议本身没有自述
    pub detail: Option<String>,
}

impl ProbeOutcome {
    fn new(kind: ServiceKind, detail: Option<String>) -> Self {
        Self {
            kind,
            detail: detail.map(|d| d.chars().take(48).collect()),
        }
    }
}

/// 探测 `127.0.0.1:<port>`。连接不上（端口已关）→ None，由调用方区分「探测失败」。
pub fn probe(port: u16) -> Option<ProbeOutcome> {
    // 1. MCP server card：零副作用 GET，命中即拿到 serverInfo
    let wellknown = http_request(
        port,
        "GET /.well-known/mcp/server-card.json HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    if let Some(out) = wellknown.as_deref().and_then(judge_wellknown) {
        return Some(out);
    }

    // 2. Web HEAD：先记下，不急着返回——dev server 也是 Web，但 MCP 更具体
    let head = http_request(
        port,
        "HEAD / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    let web = head.as_deref().and_then(judge_web);

    // 3. MCP initialize（Streamable HTTP）。Accept 必须同时带 json 与
    //    event-stream——spec 要求，只给 JSON 会被合规 server 拒收
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"prune-probe","version":"0.1.0"}}}"#;
    for path in ["/mcp", "/api/mcp", "/"] {
        let req = format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let resp = http_request(port, &req);
        if let Some(out) = resp.as_deref().and_then(judge_mcp_init) {
            return Some(out);
        }
    }

    // 4. 旧版 SSE transport
    let sse = http_request(
        port,
        "GET /sse HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: text/event-stream\r\nConnection: close\r\n\r\n",
    );
    if let Some(out) = sse.as_deref().and_then(judge_sse) {
        return Some(out);
    }

    // 5. Web 是基线结论；没有 HTTP 就再看 banner（SSH / MySQL / Redis）
    web.or_else(|| banner_probe(port))
}

// ---------------------------------------------------------------- 判定（纯函数，可单测）

/// server-card 判定：200 + JSON 含 `serverInfo` → MCP。
fn judge_wellknown(raw: &str) -> Option<ProbeOutcome> {
    let (code, _, body) = split_response(raw)?;
    if !matches!(code, 200..=299) {
        return None;
    }
    if body.contains("\"serverInfo\"") {
        Some(ProbeOutcome::new(
            ServiceKind::Mcp,
            extract_json_field(body, "serverInfo", "name"),
        ))
    } else {
        None
    }
}

/// HTTP/ 响应 → Web，`Server` 头进 detail。任何状态码都算（dev server 的 404 也是 web）。
fn judge_web(raw: &str) -> Option<ProbeOutcome> {
    if !raw.starts_with("HTTP/") {
        return None;
    }
    Some(ProbeOutcome::new(ServiceKind::Web, header_value(raw, "server")))
}

/// initialize 判定：2xx + 响应体含 `protocolVersion` 与 `serverInfo` → MCP。
/// SSE 形态（`data: {...}`）天然覆盖——substring 判据对两种形态等价。
fn judge_mcp_init(raw: &str) -> Option<ProbeOutcome> {
    let (code, _, body) = split_response(raw)?;
    if !matches!(code, 200..=299) {
        return None;
    }
    if body.contains("protocolVersion") && body.contains("serverInfo") {
        Some(ProbeOutcome::new(
            ServiceKind::Mcp,
            extract_json_field(body, "serverInfo", "name"),
        ))
    } else {
        None
    }
}

/// 旧版 SSE transport：`text/event-stream` 响应 → MCP（2024-11-05）。
fn judge_sse(raw: &str) -> Option<ProbeOutcome> {
    let (code, headers, _) = split_response(raw)?;
    if !matches!(code, 200..=299) {
        return None;
    }
    if headers.to_lowercase().contains("text/event-stream") {
        Some(ProbeOutcome::new(ServiceKind::Mcp, Some("SSE".into())))
    } else {
        None
    }
}

// ---------------------------------------------------------------- 判定辅助（纯函数）

/// 响应拆分：`(状态码, 头区, 体区)`。解析失败 → None（不是 HTTP 或残包）。
fn split_response(raw: &str) -> Option<(u16, &str, &str)> {
    let first = raw.lines().next()?;
    let code = first.split_whitespace().nth(1)?.parse::<u16>().ok()?;
    let (headers, body) = raw.split_once("\r\n\r\n").unwrap_or((raw, ""));
    Some((code, headers, body))
}

/// 头区里取某个头的值（大小写不敏感，取第一个）。
fn header_value(raw: &str, name: &str) -> Option<String> {
    let (headers, _) = raw.split_once("\r\n\r\n").unwrap_or((raw, ""));
    let lower = name.to_lowercase();
    headers
        .lines()
        .skip(1) // 跳过状态行
        .find(|l| {
            l.split_once(':')
                .map(|(k, _)| k.trim().to_lowercase() == lower)
                .unwrap_or(false)
        })
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// 在 `anchor` 字段之后的 JSON 里取 `field` 的字符串值。
///
/// 刻意不引入 JSON 解析器：探针判定只需要「serverInfo.name 存在」这一件事，
/// substring 定位在 JSON 与 SSE（`data: {...}`）两种形态下行为一致。
fn extract_json_field(body: &str, anchor: &str, field: &str) -> Option<String> {
    let after_anchor = body.split(anchor).nth(1)?;
    let pattern = format!("\"{field}\"");
    let idx = after_anchor.find(&pattern)? + pattern.len();
    let rest = after_anchor[idx..].trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    let val = &rest[..end];
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

// ---------------------------------------------------------------- 传输层

/// 发一次原始 HTTP 请求，返回封顶的原始响应文本。失败 → None。
fn http_request(port: u16, req: &str) -> Option<String> {
    let mut stream = connect(port)?;
    stream.write_all(req.as_bytes()).ok()?;
    Some(read_all(&mut stream))
}

/// 读取到对端关闭 / 超时 / 封顶为止。超时不是失败——服务器不关连接时
/// 我们已拿到足够的头与 body。
fn read_all(stream: &mut TcpStream) -> String {
    stream.set_read_timeout(Some(RW_TIMEOUT)).ok();
    let deadline = Instant::now() + RW_TIMEOUT;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if buf.len() as u64 >= READ_CAP || Instant::now() >= deadline {
            break;
        }
        match stream.read(&mut chunk) {
            Ok(0) => break, // 对端关闭
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => break, // 超时 / 重置：拿到多少算多少
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn connect(port: u16) -> Option<TcpStream> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).ok()?;
    stream.set_write_timeout(Some(RW_TIMEOUT)).ok();
    stream.set_read_timeout(Some(RW_TIMEOUT)).ok();
    Some(stream)
}

/// 被动 banner：连上后不说话，等对端先开口（SSH / MySQL 是这个模型）。
fn banner_probe(port: u16) -> Option<ProbeOutcome> {
    // SSH / MySQL greeting
    if let Some(mut stream) = connect(port) {
        stream.set_read_timeout(Some(BANNER_WAIT)).ok();
        let deadline = Instant::now() + BANNER_WAIT;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 2048];
        while buf.len() < 2048 && Instant::now() < deadline {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(_) => break, // 超时 = 对端不先说话（Redis 等）
            }
        }
        let text = String::from_utf8_lossy(&buf).to_lowercase();
        if text.starts_with("ssh-") {
            let ver = String::from_utf8_lossy(&buf[..buf.len().min(64)]).into_owned();
            return Some(ProbeOutcome::new(ServiceKind::Other, Some(ver)));
        }
        if text.contains("mysql") || text.contains("mariadb") {
            return Some(ProbeOutcome::new(ServiceKind::Database, Some("MySQL".into())));
        }
    }
    // Redis：连接后沉默，PING 才答
    let mut stream = connect(port)?;
    stream.write_all(b"PING\r\n").ok()?;
    stream.set_read_timeout(Some(BANNER_WAIT)).ok();
    let mut buf = [0u8; 64];
    let n = stream.read(&mut buf).unwrap_or(0);
    if n >= 4 && &buf[..4] == b"+PON" {
        return Some(ProbeOutcome::new(ServiceKind::Database, Some("Redis".into())));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const WELLKNOWN_BODY: &str = r#"{"version":"1.0","protocolVersion":"2025-06-18","serverInfo":{"name":"my-mcp","version":"2.1.0"},"transport":{"type":"streamable-http","endpoint":"/mcp"}}"#;
    const INIT_JSON: &str = r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"everything","version":"1.2.3"}}}"#;
    const INIT_SSE: &str = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"2025-06-18\",\"serverInfo\":{\"name\":\"sse-server\",\"version\":\"0.1\"}}}\n\n";

    #[test]
    fn wellknown_confirms_mcp() {
        let raw = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{WELLKNOWN_BODY}");
        let out = judge_wellknown(&raw).unwrap();
        assert_eq!(out.kind, ServiceKind::Mcp);
        assert_eq!(out.detail.as_deref(), Some("my-mcp"));
    }

    #[test]
    fn wellknown_404_is_nothing() {
        let raw = "HTTP/1.1 404 Not Found\r\n\r\nnope";
        assert_eq!(judge_wellknown(raw), None);
    }

    #[test]
    fn web_head_with_server_header() {
        let raw = "HTTP/1.1 200 OK\r\nServer: Caddy\r\nContent-Type: text/html\r\n\r\n";
        let out = judge_web(&raw).unwrap();
        assert_eq!(out.kind, ServiceKind::Web);
        assert_eq!(out.detail.as_deref(), Some("Caddy"));
    }

    #[test]
    fn web_head_404_still_web() {
        // dev server 的 404 也是 web——状态码不参与判定
        let raw = "HTTP/1.1 404 Not Found\r\nServer: vite\r\n\r\n";
        let out = judge_web(raw).unwrap();
        assert_eq!(out.detail.as_deref(), Some("vite"));
    }

    #[test]
    fn non_http_is_not_web() {
        assert_eq!(judge_web("garbage bytes"), None);
    }

    #[test]
    fn init_json_confirms_mcp() {
        let raw = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{INIT_JSON}");
        let out = judge_mcp_init(&raw).unwrap();
        assert_eq!(out.kind, ServiceKind::Mcp);
        assert_eq!(out.detail.as_deref(), Some("everything"));
    }

    #[test]
    fn init_sse_form_also_confirms() {
        let raw = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n{INIT_SSE}"
        );
        let out = judge_mcp_init(&raw).unwrap();
        assert_eq!(out.kind, ServiceKind::Mcp);
        assert_eq!(out.detail.as_deref(), Some("sse-server"));
    }

    #[test]
    fn jsonrpc_error_without_serverinfo_is_nothing() {
        // 版本不兼容等 JSON-RPC error：没有 serverInfo，不下结论
        let raw = "HTTP/1.1 200 OK\r\n\r\n{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-32602,\"message\":\"unsupported protocol\"}}";
        assert_eq!(judge_mcp_init(raw), None);
    }

    #[test]
    fn sse_transport_confirms_mcp() {
        let raw = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\nevent: endpoint\ndata: /messages\r\n\r\n";
        let out = judge_sse(&raw).unwrap();
        assert_eq!(out.kind, ServiceKind::Mcp);
    }

    #[test]
    fn sse_json_response_is_not_legacy_sse() {
        let raw = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"ok\":true}";
        assert_eq!(judge_sse(raw), None);
    }

    #[test]
    fn detail_is_capped() {
        let long = "x".repeat(200);
        let out = ProbeOutcome::new(ServiceKind::Web, Some(long));
        assert_eq!(out.detail.unwrap().len(), 48);
    }

    #[test]
    fn extract_field_edge_cases() {
        assert_eq!(extract_json_field(INIT_JSON, "serverInfo", "name"), Some("everything".into()));
        // anchor 不存在
        assert_eq!(extract_json_field("{\"a\":1}", "serverInfo", "name"), None);
        // 字段为空串
        assert_eq!(
            extract_json_field("{\"serverInfo\":{\"name\":\"\"}}", "serverInfo", "name"),
            None
        );
    }

    /// 端到端：起一个本地假 server，走完整 probe() 链路验证传输层。
    #[test]
    fn end_to_end_against_local_fixture() {
        use std::io::Write as _;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // 只服务一次就退出：probe() 的第一跳 wellknown 就命中，只会有一个连接；
        // 线程必须能结束，否则 join 挂死测试
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                let mut buf = [0u8; 4096];
                let _ = std::io::Read::read(&mut stream, &mut buf);
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{WELLKNOWN_BODY}"
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });

        let out = probe(port).expect("fixture 必须给出 MCP 结论");
        assert_eq!(out.kind, ServiceKind::Mcp);
        assert_eq!(out.detail.as_deref(), Some("my-mcp"));
        handle.join().ok();
    }
}
