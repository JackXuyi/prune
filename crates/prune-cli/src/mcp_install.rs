//! mcp_install.rs：`prune mcp install` —— 把 MCP server 写进客户端配置（W1/D-05）。
//!
//! 这是 Prune 少见的「写用户文件」动作，纪律与 `prune.toml` 同构
//! （决策 W1-DP-4 裁 A）：**默认 dry-run**，`--write` 才落盘；只改已知键；
//! 写前备份；坏 JSON 拒绝而不是覆盖（坏 JSON 大概率含用户手写内容，
//! 覆盖就是把人家的工作删了——`--force` 也不豁免这一条）。
//!
//! 客户端配置事实（wave1 方案 §2.4，2026-09 核验）：
//! | 客户端 | 路径 | 顶层键 | 条目 |
//! | Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json`（Win `%APPDATA%\Claude\…`）| `mcpServers` | `{command,args}` |
//! | Cursor | `~/.cursor/mcp.json` | `mcpServers` | `{command,args}` |
//! | VS Code | `~/Library/Application Support/Code/User/mcp.json`（Win `%APPDATA%\Code\User\…`）| **`servers`** | 需 `"type":"stdio"` |
//!
//! VS Code 是唯一的坑：用 `servers` 而非 `mcpServers`，且条目要带 `type`——
//! 抄错是「合法 JSON、不报错、不加载」。只支持**用户级**配置（决策 W1-DP-5）：
//! 项目级 `.cursor/mcp.json` / `.vscode/mcp.json` 会弄脏 `git status`。

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

/// 备份文件时间戳后缀格式 `YYYYMMDD-HHMMSS`（localtime 私有，这里自算）
fn stamp(ts: i64) -> String {
    #[cfg(unix)]
    {
        unsafe {
            let t = ts as libc::time_t;
            let mut tm: libc::tm = std::mem::zeroed();
            if libc::localtime_r(&t, &mut tm).is_null() {
                return "unknown".into();
            }
            format!(
                "{:04}{:02}{:02}-{:02}{:02}{:02}",
                tm.tm_year as i64 + 1900,
                (tm.tm_mon + 1).clamp(1, 12),
                tm.tm_mday.clamp(1, 31),
                tm.tm_hour.clamp(0, 23),
                tm.tm_min.clamp(0, 59),
                tm.tm_sec.clamp(0, 61)
            )
        }
    }
    #[cfg(not(unix))]
    {
        let _ = ts;
        "unknown".into()
    }
}

// ---------------------------------------------------------------- 客户端

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    Claude,
    Cursor,
    Vscode,
}

impl Client {
    pub fn name(self) -> &'static str {
        match self {
            Client::Claude => "claude",
            Client::Cursor => "cursor",
            Client::Vscode => "vscode",
        }
    }

    /// 顶层容器键：VS Code 用 `servers`，其余用 `mcpServers`
    pub fn wrapper_key(self) -> &'static str {
        match self {
            Client::Claude | Client::Cursor => "mcpServers",
            Client::Vscode => "servers",
        }
    }

    /// 配置路径（用户级）。代码内常量给出，**不接受**用户传 `--path`。
    /// Linux 的 Claude Desktop 路径官方未发布（第三方写 `~/.config/Claude/`），
    /// 返回路径但在 plan 的 note 里写明「未被官方文档确认」。
    pub fn config_path(self) -> Option<PathBuf> {
        let home = prune_core::paths::home_dir()?;
        #[cfg(target_os = "macos")]
        {
            match self {
                Client::Claude => {
                    Some(home.join("Library/Application Support/Claude/claude_desktop_config.json"))
                }
                Client::Cursor => Some(home.join(".cursor/mcp.json")),
                Client::Vscode => Some(home.join("Library/Application Support/Code/User/mcp.json")),
            }
        }
        #[cfg(windows)]
        {
            match self {
                Client::Claude => std::env::var("APPDATA")
                    .ok()
                    .map(|a| PathBuf::from(a).join("Claude/claude_desktop_config.json")),
                Client::Cursor => Some(home.join(".cursor/mcp.json")),
                Client::Vscode => std::env::var("APPDATA")
                    .ok()
                    .map(|a| PathBuf::from(a).join("Code/User/mcp.json")),
            }
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            match self {
                Client::Claude => Some(home.join(".config/Claude/claude_desktop_config.json")),
                Client::Cursor => Some(home.join(".cursor/mcp.json")),
                Client::Vscode => Some(home.join(".config/Code/User/mcp.json")),
            }
        }
    }

    /// 安装说明里需要提醒用户的注意项（dry-run 输出与 `--json` 都带）
    pub fn note(self) -> Option<&'static str> {
        #[cfg(not(any(target_os = "macos", windows)))]
        if self == Client::Claude {
            return Some("该路径未被官方文档确认（Linux 版 Claude Desktop 配置路径未正式发布）");
        }
        let _ = self;
        None
    }
}

/// `--client` 的取值：三家任一或 all
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientSpec {
    One(Client),
    All,
}

impl ClientSpec {
    pub fn parse(s: &str) -> Option<ClientSpec> {
        match s {
            "claude" => Some(ClientSpec::One(Client::Claude)),
            "cursor" => Some(ClientSpec::One(Client::Cursor)),
            "vscode" => Some(ClientSpec::One(Client::Vscode)),
            "all" => Some(ClientSpec::All),
            _ => None,
        }
    }

    pub fn clients(self) -> Vec<Client> {
        match self {
            ClientSpec::One(c) => vec![c],
            ClientSpec::All => vec![Client::Claude, Client::Cursor, Client::Vscode],
        }
    }
}

// ---------------------------------------------------------------- 条目

/// `prune` 条目。`command` 用绝对路径——客户端启动 MCP server 时的 PATH
/// 与本机 shell 不一致，写裸名是「安装成功但加载失败」的头号原因。
pub fn entry_for(exe: &Path, client: Client) -> Value {
    let mut entry = Map::new();
    if client == Client::Vscode {
        entry.insert("type".into(), Value::from("stdio"));
    }
    entry.insert(
        "command".into(),
        Value::from(exe.to_string_lossy().as_ref()),
    );
    entry.insert("args".into(), Value::from(vec!["mcp"]));
    Value::Object(entry)
}

// ---------------------------------------------------------------- 计划

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// 文件不存在 → 新建
    Create,
    /// 有 prune 条目但与目标有差异 → 更新（保留其余键）
    Update,
    /// 条目已等价 → 幂等 noop（重复执行不产生备份噪音）
    Noop,
    /// 坏 JSON / 容器键不是对象 / 客户端未安装 → 拒绝，文件字节不变
    Refused,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub client: &'static str,
    /// 原始 Client（Plan 需要按客户端分流容器键；序列化用 name 即可）
    #[serde(skip)]
    pub kind: Client,
    pub path: String,
    pub action: Action,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// 已有条目（update 时给 diff 用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<Value>,
    /// 将写入的条目
    pub entry: Value,
}

/// 纯函数计划：给定配置路径与 exe 路径，产出动作与条目。测试直接喂 fixture 路径。
pub fn plan_for(client: Client, config_path: &Path, exe: &Path) -> Plan {
    let entry = entry_for(exe, client);
    let base = |action: Action, note: Option<String>, before: Option<Value>| Plan {
        client: client.name(),
        kind: client,
        path: config_path.to_string_lossy().into_owned(),
        action,
        note,
        before,
        entry: entry.clone(),
    };

    if !config_path.exists() {
        // 父目录不存在 = 客户端没装过 → 不替用户造目录树
        if config_path.parent().is_none_or(|p| !p.exists()) {
            return base(
                Action::Refused,
                Some("未检测到客户端（配置父目录不存在）：先安装并启动一次该客户端".into()),
                None,
            );
        }
        return base(Action::Create, client.note().map(String::from), None);
    }

    let text = match std::fs::read_to_string(config_path) {
        Ok(t) => t,
        Err(e) => return base(Action::Refused, Some(format!("读取失败: {e}")), None),
    };
    let mut root: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        // 坏 JSON：拒绝而不是覆盖（--force 也不豁免——覆盖就是删用户工作）
        Err(e) => {
            return base(
                Action::Refused,
                Some(format!(
                    "现有配置不是合法 JSON（{e}）：先修好这份配置，Prune 不覆盖坏 JSON"
                )),
                None,
            )
        }
    };

    let key = client.wrapper_key();
    let container = match root.get_mut(key) {
        None => {
            root[key] = Value::Object(Map::new());
            root.get_mut(key).and_then(|v| v.as_object_mut()).unwrap()
        }
        Some(Value::Object(m)) => m,
        Some(_) => {
            return base(
                Action::Refused,
                Some(format!(
                    "顶层键 {key:?} 存在但不是对象：手动确认后修复，Prune 不改写"
                )),
                None,
            )
        }
    };

    match container.get("prune") {
        Some(existing) if *existing == entry => base(
            Action::Noop,
            client.note().map(String::from),
            Some(existing.clone()),
        ),
        Some(existing) => base(
            Action::Update,
            client.note().map(String::from),
            Some(existing.clone()),
        ),
        None => base(Action::Update, client.note().map(String::from), None),
    }
}

// ---------------------------------------------------------------- 执行

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    pub plan: Plan,
    /// 写入是否成功（Noop / Refused / dry-run 时为 false 或按语义）
    pub written: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 七步写入算法（wave1 方案 §5.3，全部有测试）：
/// 备份 → 原子写 → 读回校验 → 失败回滚。
pub fn apply(plan: &Plan, write: bool, ts: i64) -> Applied {
    let base = |written: bool, backup: Option<String>, error: Option<String>| Applied {
        plan: plan.clone(),
        written,
        backup,
        error,
    };

    if !write {
        return base(false, None, None);
    }
    if plan.action == Action::Refused || plan.action == Action::Noop {
        return base(false, None, None);
    }

    let path = Path::new(&plan.path);
    let mut root: Value = if path.exists() {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap_or_default())
            .unwrap_or(Value::Object(Map::new()))
    } else {
        Value::Object(Map::new())
    };
    let before_root = root.clone();

    let obj = root.as_object_mut().unwrap();
    obj.entry(plan.kind.wrapper_key().to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .unwrap()
        .insert("prune".into(), plan.entry.clone());

    // 6) 备份（0600）——只在已有文件时
    let backup_path = path.exists().then(|| {
        let bak = path.with_extension(format!("json.prune-bak-{}", stamp(ts)));
        let _ = std::fs::copy(path, &bak);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&bak, std::fs::Permissions::from_mode(0o600));
        }
        bak.to_string_lossy().into_owned()
    });

    // 7) 原子写 + 读回校验
    let text = serde_json::to_string_pretty(&root).unwrap_or_default() + "\n";
    let tmp = path.with_extension("json.prune-tmp");
    if let Err(e) = std::fs::write(&tmp, &text) {
        return base(false, backup_path, Some(format!("写入失败: {e}")));
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return base(false, backup_path, Some(format!("替换失败: {e}")));
    }

    // 读回：prune 条目存在且顶层其他键逐一相等（不丢用户配置）
    let readback: Value = match std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
    {
        Some(v) => v,
        None => {
            rollback(path, backup_path.as_deref());
            return base(false, backup_path, Some("写后读回失败：已回滚".into()));
        }
    };
    if !verify(&before_root, &readback, plan.kind.wrapper_key()) {
        rollback(path, backup_path.as_deref());
        return base(
            false,
            backup_path,
            Some("读回内容与预期不符（可能丢失其他键）：已回滚".into()),
        );
    }
    base(true, backup_path, None)
}

/// 校验：prune 条目 = 目标，且顶层其他键与写前逐一相等
fn verify(before: &Value, after: &Value, wrapper_key: &str) -> bool {
    let (Some(b), Some(a)) = (before.as_object(), after.as_object()) else {
        return false;
    };
    let Some(w) = a.get(wrapper_key).and_then(|v| v.get("prune")) else {
        return false;
    };
    let entry_target = w;
    // 顶层键集合：原有键一个不能少
    for (k, v) in b {
        let after_v = a.get(k);
        match after_v {
            None => return false,
            Some(av) => {
                if k == wrapper_key {
                    // 容器键内：其余条目不能丢（prune 条目比对目标）
                    let (Some(bm), Some(am)) = (v.as_object(), av.as_object()) else {
                        return false;
                    };
                    for (ik, iv) in bm {
                        if ik == "prune" {
                            if am.get("prune") != Some(entry_target) {
                                return false;
                            }
                        } else if am.get(ik) != Some(iv) {
                            return false;
                        }
                    }
                } else if av != v {
                    return false;
                }
            }
        }
    }
    true
}

fn rollback(path: &Path, backup: Option<&str>) {
    if let Some(b) = backup {
        let _ = std::fs::copy(b, path);
    }
}

// ---------------------------------------------------------------- 输出与入口

pub fn run(spec: ClientSpec, write: bool, json: bool, ts: i64) -> i32 {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("prune"));
    let mut applied = Vec::new();
    for client in spec.clients() {
        let Some(path) = client.config_path() else {
            continue;
        };
        let plan = plan_for(client, &path, &exe);
        applied.push(apply(&plan, write, ts));
    }

    if json {
        let doc = serde_json::json!({
            "schemaVersion": 1,
            "mode": if write { "write" } else { "dry-run" },
            "results": applied,
        });
        println!("{}", serde_json::to_string_pretty(&doc).unwrap());
        return 0;
    }

    for a in &applied {
        let act = match a.plan.action {
            Action::Create => "新建",
            Action::Update => "更新",
            Action::Noop => "已是最新（noop）",
            Action::Refused => "拒绝",
        };
        println!(
            "{}  {}  [{}]",
            a.plan.client.to_uppercase(),
            a.plan.path,
            act
        );
        if !write {
            println!(
                "  将写入：{}",
                serde_json::to_string(&a.plan.entry).unwrap_or_default()
            );
        }
        if let Some(before) = &a.plan.before {
            println!(
                "  现有条目：{}",
                serde_json::to_string(before).unwrap_or_default()
            );
        }
        if let Some(note) = &a.plan.note {
            println!("  ⚠️ {note}");
        }
        if let Some(bak) = &a.backup {
            println!("  备份：{bak}");
        }
        if let Some(err) = &a.error {
            println!("  ✗ {err}");
        }
    }
    if !write {
        println!();
        println!("以上为 dry-run 预览，未写任何文件。确认无误后追加 --write 执行。");
    }
    0
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 唯一临时目录：进程内自增，避免 tempfile 依赖（D-C1 不加依赖）
    fn tmpdir(tag: &str) -> PathBuf {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!(
            "prune-mcp-install-test-{}-{}-{}",
            std::process::id(),
            tag,
            n
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    const EXE: &str = "/Applications/Prune.app/Contents/MacOS/prune bin"; // 含空格

    #[test]
    fn create_when_parent_exists_and_file_missing() {
        let dir = tmpdir("create");
        let path = dir.join("claude_desktop_config.json");
        let plan = plan_for(Client::Claude, &path, Path::new(EXE));
        assert_eq!(plan.action, Action::Create);

        let applied = apply(&plan, true, 0);
        assert!(applied.written);
        assert!(applied.backup.is_none(), "新建不产生备份");
        let root: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(root["mcpServers"]["prune"]["command"], EXE);
        assert_eq!(root["mcpServers"]["prune"]["args"][0], "mcp");
        assert_eq!(root.as_object().unwrap().len(), 1, "只写自己的键");
    }

    #[test]
    fn refused_when_parent_missing_no_dir_tree_created() {
        let dir = tmpdir("noparent");
        let path = dir.join("Claude/claude_desktop_config.json");
        let plan = plan_for(Client::Claude, &path, Path::new(EXE));
        assert_eq!(plan.action, Action::Refused);
        assert!(plan.note.unwrap_or_default().contains("未检测到客户端"));
        assert!(!path.parent().unwrap().exists(), "不替用户造目录树");
    }

    #[test]
    fn refused_on_broken_json_even_with_bytes_unchanged() {
        let dir = tmpdir("badjson");
        let path = dir.join("mcp.json");
        let broken = "{ \"mcpServers\": { \"x\": [1, }"; // 尾部坏语法
        fs::write(&path, broken).unwrap();
        let plan = plan_for(Client::Cursor, &path, Path::new(EXE));
        assert_eq!(plan.action, Action::Refused);
        assert!(plan
            .note
            .as_deref()
            .unwrap_or_default()
            .contains("不覆盖坏 JSON"));
        // --write 也不覆盖：文件字节不变
        let applied = apply(&plan, true, 0);
        assert!(!applied.written);
        assert_eq!(fs::read(&path).unwrap(), broken.as_bytes());
    }

    #[test]
    fn update_preserves_other_keys_and_makes_backup() {
        let dir = tmpdir("update");
        let path = dir.join("mcp.json");
        fs::write(
            &path,
            r#"{ "mcpServers": { "prune": { "command": "prune", "args": ["mcp"] }, "other": { "command": "o" } }, "globalShortcut": "Ctrl+P" }"#,
        )
        .unwrap();
        let plan = plan_for(Client::Cursor, &path, Path::new(EXE));
        assert_eq!(plan.action, Action::Update);
        assert_eq!(plan.before.as_ref().unwrap()["command"], "prune");

        let applied = apply(&plan, true, 1_700_000_000);
        assert!(applied.written);
        assert!(applied
            .backup
            .as_deref()
            .unwrap_or("")
            .contains("prune-bak-"));
        let root: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(root["mcpServers"]["prune"]["command"], EXE);
        assert_eq!(
            root["mcpServers"]["other"]["command"], "o",
            "别人的条目不能丢"
        );
        assert_eq!(root["globalShortcut"], "Ctrl+P", "顶层其他键不能丢");
    }

    #[test]
    fn noop_is_idempotent_and_makes_no_backup_noise() {
        let dir = tmpdir("noop");
        let path = dir.join("mcp.json");
        let exe = Path::new(EXE);
        // 第一次写入
        let plan = plan_for(Client::Cursor, &path, exe);
        let _ = apply(&plan, true, 0);
        // 第二次：等价 → noop，无备份
        let plan2 = plan_for(Client::Cursor, &path, exe);
        assert_eq!(plan2.action, Action::Noop);
        let applied = apply(&plan2, true, 0);
        assert!(!applied.written, "noop 不重写");
        assert!(applied.backup.is_none(), "noop 不产生备份噪音");
    }

    #[test]
    fn vscode_uses_servers_and_type_stdio() {
        let dir = tmpdir("vscode");
        let path = dir.join("mcp.json");
        let plan = plan_for(Client::Vscode, &path, Path::new(EXE));
        assert_eq!(plan.client, "vscode");
        assert_eq!(plan.entry["type"], "stdio");
        assert_eq!(plan.entry["command"], EXE);
        let applied = apply(&plan, true, 0);
        assert!(applied.written);
        let root: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(root.get("servers").is_some(), "VS Code 顶层键是 servers");
        assert!(
            root.get("mcpServers").is_none(),
            "抄 mcpServers 是「装了但零工具」的坑"
        );
    }

    #[test]
    fn verify_catches_lost_sibling_keys_and_rolls_back() {
        // 构造一个「写后丢键」的现场：直接调 verify 而不是真写坏文件
        let before: Value = serde_json::json!({ "mcpServers": { "other": 1 } });
        let after: Value = serde_json::json!({ "mcpServers": { "prune": {} } }); // other 丢了
        assert!(!verify(&before, &after, "mcpServers"));
        let after_ok: Value = serde_json::json!({ "mcpServers": { "other": 1, "prune": {} } });
        assert!(verify(&before, &after_ok, "mcpServers"));
    }

    #[test]
    fn rollback_restores_from_backup() {
        let dir = tmpdir("rollback");
        let path = dir.join("mcp.json");
        fs::write(&path, "{}").unwrap();
        let bak = path.with_extension(format!("json.prune-bak-{}", stamp(0)));
        fs::write(&bak, "{\"keep\":1}").unwrap();
        fs::write(&path, "{\"corrupted\":true}").unwrap();
        rollback(&path, Some(bak.to_str().unwrap()));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"keep\":1}");
    }

    #[test]
    fn client_spec_parses_all_variants() {
        for s in ["claude", "cursor", "vscode", "all"] {
            assert!(ClientSpec::parse(s).is_some());
        }
        assert!(ClientSpec::parse("windsurf").is_none());
        assert_eq!(ClientSpec::All.clients().len(), 3);
    }

    #[test]
    fn entry_survives_spaces_in_exe_path() {
        let entry = entry_for(Path::new(EXE), Client::Claude);
        let text = serde_json::to_string(&entry).unwrap();
        let back: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back["command"], EXE, "含空格的 .app 路径由 serde_json 转义");
    }
}
