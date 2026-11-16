//! `prune.toml`（W2/D-09）：项目级配置，**只能收窄，不能放宽**。
//!
//! 安全边界（方案 §6.1，先于实现定下）：
//! - 允许的键就这五个：`ignore.projects` / `ignore.paths` / `thresholds.deps_warn_gb` /
//!   `thresholds.stale_days` / `watch.ports`——配置面本身走白名单纪律；
//! - 任何试图**扩大**范围的键（`extra_paths` / `clean` / `root` / `home` /
//!   `[reclaim]` / `[auto]` 段）解析期直接拒绝，不静默忽略。白名单的信任来自
//!   「路径是数据文件 + 每条有断言测试」，允许用户配置新增路径，一次写错就是
//!   一次数据丢失，而 blame 会落在 Prune 上；
//! - 语法超出子集（嵌套表 / 多行字符串 / 日期 / 裸布尔）→ 「第 N 行不支持该语法」
//!   ——明确拒绝好过悄悄解析错。
//!
//! 解析器是手写的极简 TOML 子集（方案 §6.2）：`toml` crate 虽已在依赖树里，但让
//! prune-core 依赖它会把完整解析器拉进 CLI 二进制——CLI 的纪律是「手写 arg 不引
//! clap，因为拖慢编译且无必要」（main.rs D-C1），同一纪律适用于这里。
//!
//! 合并规则（§6.3）：
//! - 项目无 `prune.toml` → 行为与无配置完全一致；
//! - 文件语法错 / 超子集 / 禁用键 → **整份忽略** + note；
//! - 键合法但值越界 → **该键回落默认** + note（不整份丢弃）；
//! - 只读项目根那一-份，子包的 `prune.toml` 不生效（多份合并的优先级无法向用户解释）；
//! - Prune 永不写项目文件——模板由设置页复制到剪贴板，用户自己落盘。

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 项目根下的配置文件名
pub const FILE_NAME: &str = "prune.toml";

/// 设置页「复制模板」用的模板（单语中文，与 CLI 文案同一约定）。
/// 唯一真源在核心侧——GUI 与 CLI 将来若都要用，不会出现两份漂移。
/// 必须能被 `parse()` 干净解析（见 `template_parses_cleanly` 测试）。
pub const TEMPLATE: &str = r#"# Prune 项目配置（放到项目根下生效；只能收窄范围，不能扩大）
# 语法只支持 [段] + key = value（字符串 / 数字 / 字符串数组），详见设置页说明

[ignore]
# 跳过这些项目：不扫描、不进体检、不进依赖账本（≤ 200 条，绝对路径或 ~ 开头）
projects = []
# 从清理候选中排除这些路径（只能写白名单目录的子路径）
paths = []

[thresholds]
# 依赖「偏胖」阈值（GB），默认 1.5；取值 0.1 – 100
#deps_warn_gb = 2.0
# 「久未重装」天数，默认 60；取值 7 – 3650
#stale_days = 90

[watch]
# 本项目的关注端口（并入端口页关注层；≤ 32 个，1 – 65535）
ports = []
"#;

// -------------------------------------------------------------- 上限与取值域

/// `ignore.projects` / `ignore.paths` 条目上限
pub const IGNORE_MAX: usize = 200;
/// `watch.ports` 上限
pub const WATCH_PORTS_MAX: usize = 32;
/// `thresholds.deps_warn_gb` 取值域
pub const DEPS_WARN_GB_MIN: f64 = 0.1;
pub const DEPS_WARN_GB_MAX: f64 = 100.0;
/// `thresholds.stale_days` 取值域
pub const STALE_DAYS_MIN: u64 = 7;
pub const STALE_DAYS_MAX: u64 = 3650;

// -------------------------------------------------------------- 数据结构

/// 一个项目的生效配置。`None` 字段 = 用户没写，用内置默认。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectConfig {
    /// 跳过这些项目（不扫、不进账本）。原始字符串（`~` 未展开）
    pub ignore_projects: Vec<String>,
    /// 从清理候选中排除这些路径。原始字符串（`~` 未展开）
    pub ignore_paths: Vec<String>,
    /// 依赖「偏胖」阈值（覆盖 1.5 GiB 默认）
    pub deps_warn_gb: Option<f64>,
    /// 「久未重装」天数（覆盖 60 天默认）
    pub stale_days: Option<u64>,
    /// 本项目的关注端口（并入关注层）
    pub watch_ports: Vec<u16>,
}

impl ProjectConfig {
    /// 偏胖阈值的字节口径（用户写了就用用户的，否则内置默认）
    pub fn heavy_bytes(&self) -> u64 {
        match self.deps_warn_gb {
            Some(gb) => (gb * 1024.0 * 1024.0 * 1024.0) as u64,
            None => crate::disk::project::PROJECT_HEAVY_BYTES,
        }
    }

    /// 「久未重装」天数（用户写了就用用户的，否则内置默认）
    pub fn stale_days_or_default(&self) -> u64 {
        self.stale_days
            .unwrap_or(crate::disk::project::PROJECT_STALE_DAYS)
    }
}

/// note 种类。前端按 kind 出本地化文案，Rust 侧不存整句（GUI 是双语的）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigNoteKind {
    /// 整份回落默认（语法错 / 超子集 / 禁用键）
    Syntax,
    /// 键合法但值越界，该键回落默认
    Value,
    /// `ignore.paths` 条目不在白名单内，已跳过
    Path,
    /// 项目被 `ignore.projects` 跳过
    Ignored,
}

/// 一条配置事件。UI 与 CLI 据此提示「发生了什么、实际生效的是什么」。
/// Deserialize：随 DiskReport 落盘 / 反序列化（旧报告缺省为 None）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigNote {
    pub kind: ConfigNoteKind,
    /// 配置文件绝对路径
    pub file: String,
    /// 语法/禁用/越界发生的行；文件级事件为 None
    pub line: Option<u32>,
    /// 补充事实：键名 / 被跳过的路径 / 被跳过的项目名
    pub detail: Option<String>,
}

/// 整份拒绝的错误。`line = None` 表示文件级（读不了）。
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigError {
    pub line: Option<u32>,
    pub reason: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(l) => write!(f, "{FILE_NAME} 第 {l} 行：{}", self.reason),
            None => write!(f, "{FILE_NAME}：{}", self.reason),
        }
    }
}

/// 解析成功的产出：配置 + 值级回落 note（语法错走 `Err`，不会出现在 notes 里）
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub config: ProjectConfig,
    pub notes: Vec<ConfigNote>,
}

// -------------------------------------------------------------- 入口

/// 读 `<root>/prune.toml`。`Ok(None)` = 没有配置文件（零配置即默认）。
pub fn load_at(path: &Path) -> Result<Option<Parsed>, ConfigError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(ConfigError {
                line: None,
                reason: format!("读取失败（{e}），整份忽略"),
            })
        }
    };
    let mut parsed = parse(&text)?;
    parsed
        .notes
        .iter_mut()
        .for_each(|n| n.file = path.to_string_lossy().to_string());
    Ok(Some(parsed))
}

/// `~` / `~/x` → home 下的绝对路径；其余原样（相对路径在扫描侧按项目根解释，
/// 但白名单键只接受绝对路径或 `~` 开头，见 `validate_path_entry`）。
pub fn expand_path(p: &str, home: &Path) -> PathBuf {
    if p == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = p.strip_prefix("~/") {
        return home.join(rest);
    }
    PathBuf::from(p)
}

// -------------------------------------------------------------- 解析器

#[derive(Debug, Clone, PartialEq)]
enum Val {
    Str(String),
    Int(i64),
    Float(f64),
    Arr(Vec<Val>),
}

/// 禁用段：Prune 不接受任何自动执行配置
const BANNED_SECTIONS: &[&str] = &["reclaim", "auto", "clean"];
/// 禁用键：路径重定向
const BANNED_REDIRECT_KEYS: &[&str] = &["root", "home"];
/// 禁用键：新增可删除路径
const BANNED_EXPAND_KEYS: &[&str] = &["extra_paths", "clean", "add_paths", "include", "paths_add"];

/// 解析整份配置。语法 / 禁用项 → `Err`（整份拒绝）；值越界 → 键级回落 + note。
pub fn parse(text: &str) -> Result<Parsed, ConfigError> {
    let mut cfg = ProjectConfig::default();
    let mut notes: Vec<ConfigNote> = Vec::new();
    let mut section: Option<String> = None;
    let mut seen_sections: Vec<String> = Vec::new();
    let mut seen_keys: Vec<String> = Vec::new();

    let raw_lines: Vec<&str> = text.lines().collect();
    let mut i = 0usize;
    while i < raw_lines.len() {
        let line_no = (i + 1) as u32;
        let line = raw_lines[i].trim();
        i += 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // [section]
        if let Some(inner) = line.strip_prefix('[') {
            let Some(name) = inner.strip_suffix(']') else {
                return Err(ConfigError {
                    line: Some(line_no),
                    reason: "段头缺少右括号 ]".into(),
                });
            };
            let name = name.trim();
            if name.is_empty() || name.contains('.') || name.contains('[') || name.contains(']') {
                return Err(ConfigError {
                    line: Some(line_no),
                    reason: format!("不支持嵌套表或空段名「{name}」"),
                });
            }
            let lower = name.to_ascii_lowercase();
            if BANNED_SECTIONS.contains(&lower.as_str()) {
                return Err(ConfigError {
                    line: Some(line_no),
                    reason: format!("[{name}] 段不接受：Prune 不接受任何自动执行配置"),
                });
            }
            if seen_sections.contains(&lower) {
                return Err(ConfigError {
                    line: Some(line_no),
                    reason: format!("段 [{name}] 重复定义"),
                });
            }
            seen_sections.push(lower.clone());
            section = Some(lower);
            continue;
        }

        // key = value
        let Some((key_raw, value_raw)) = line.split_once('=') else {
            return Err(ConfigError {
                line: Some(line_no),
                reason: "不是 [段] 也不是 key = value".into(),
            });
        };
        let key = key_raw.trim();
        if key.is_empty() || key.chars().any(|c| c.is_whitespace() || c == '"') {
            return Err(ConfigError {
                line: Some(line_no),
                reason: format!("键名不合法「{key}」"),
            });
        }
        let Some(sec) = &section else {
            return Err(ConfigError {
                line: Some(line_no),
                reason: "键必须放在某个 [section] 下".into(),
            });
        };

        let full = format!("{sec}.{key}");
        // 禁用键检查（无论段名是什么——把 clean 塞进 [ignore] 也是扩大范围）
        if BANNED_REDIRECT_KEYS.contains(&key.to_ascii_lowercase().as_str()) {
            return Err(ConfigError {
                line: Some(line_no),
                reason: format!("键「{key}」不接受：不接受路径重定向"),
            });
        }
        if BANNED_EXPAND_KEYS.contains(&key.to_ascii_lowercase().as_str()) {
            return Err(ConfigError {
                line: Some(line_no),
                reason: format!(
                    "键「{key}」不接受：项目配置只能减少范围，不能扩大；可清理项由内置白名单决定"
                ),
            });
        }
        if seen_keys.contains(&full) {
            return Err(ConfigError {
                line: Some(line_no),
                reason: format!("键「{key}」在 [{sec}] 里重复定义"),
            });
        }
        seen_keys.push(full.clone());

        // 数组可能跨行：把后续行并进来，直到方括号配平
        let mut value_buf = value_raw.to_string();
        if value_buf.trim_start().starts_with('[') {
            while !brackets_closed(&value_buf) {
                let Some(next) = raw_lines.get(i) else {
                    return Err(ConfigError {
                        line: Some(line_no),
                        reason: "数组没有闭合（缺 ]）".into(),
                    });
                };
                value_buf.push(' ');
                value_buf.push_str(next.trim());
                i += 1;
            }
        }

        let val = parse_value(&value_buf, line_no)?;
        apply(&mut cfg, &mut notes, sec, key, val, line_no)?;
    }

    Ok(Parsed { config: cfg, notes })
}

/// 方括号配平检查（引号内的 `]` 不计数）
fn brackets_closed(s: &str) -> bool {
    let mut in_str: Option<char> = None;
    let mut depth = 0i32;
    let mut escaped = false;
    for c in s.chars() {
        if let Some(q) = in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' && q == '"' {
                escaped = true;
            } else if c == q {
                in_str = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => in_str = Some(c),
            '[' => depth += 1,
            ']' => depth -= 1,
            _ => {}
        }
    }
    depth <= 0 && in_str.is_none()
}

/// 值解析：字符串 / 整数 / 浮点 / 同构数组。超出子集一律报行号。
fn parse_value(raw: &str, line_no: u32) -> Result<Val, ConfigError> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(ConfigError {
            line: Some(line_no),
            reason: "缺值".into(),
        });
    }

    // 数组：允许跨行（调用方已拼接）
    if s.starts_with('[') {
        let inner = s
            .strip_prefix('[')
            .and_then(|x| x.strip_suffix(']'))
            .ok_or_else(|| ConfigError {
                line: Some(line_no),
                reason: "数组括号不配对".into(),
            })?;
        let mut items: Vec<Val> = Vec::new();
        let mut kind: Option<u8> = None; // 1=字符串 2=数字；异构数组直接拒绝
        for tok in split_array_items(inner, line_no)? {
            if tok.trim().is_empty() {
                continue; // 允许尾逗号
            }
            let v = parse_scalar(&tok, line_no)?;
            let k = match v {
                Val::Str(_) => 1u8,
                Val::Int(_) | Val::Float(_) => 2u8,
                Val::Arr(_) => {
                    return Err(ConfigError {
                        line: Some(line_no),
                        reason: "数组里嵌套数组：不支持该语法".to_string(),
                    })
                }
            };
            match kind {
                None => kind = Some(k),
                Some(x) if x != k => {
                    return Err(ConfigError {
                        line: Some(line_no),
                        reason: "数组元素类型不一致（本项目配置里没有需要异构数组的键）"
                            .to_string(),
                    })
                }
                _ => {}
            }
            items.push(v);
        }
        return Ok(Val::Arr(items));
    }

    parse_scalar(s, line_no)
}

/// 单值：带引号字符串 / 整数 / 浮点。行尾允许 `# 注释`（引号外）。
fn parse_scalar(raw: &str, line_no: u32) -> Result<Val, ConfigError> {
    let s = raw.trim();
    // 字符串：基本 "..." / 字面 '...'
    if s.starts_with('"') || s.starts_with('\'') {
        let q = s.chars().next().unwrap();
        let mut out = String::new();
        let mut chars = s.chars().enumerate().peekable();
        chars.next(); // 开引号
        let mut escaped = false;
        let mut closed = false;
        for (_, c) in chars.by_ref() {
            if escaped {
                // 子集只支持两种转义，其余原样保留（明确好过猜）
                out.push(match c {
                    'n' => '\n',
                    't' => '\t',
                    other => other,
                });
                escaped = false;
                continue;
            }
            if c == '\\' && q == '"' {
                escaped = true;
                continue;
            }
            if c == q {
                closed = true;
                break;
            }
            out.push(c);
        }
        if !closed {
            return Err(ConfigError {
                line: Some(line_no),
                reason: "字符串没有闭合".into(),
            });
        }
        let rest: String = chars.map(|(_, c)| c).collect();
        let rest = rest.trim();
        if !rest.is_empty() && !rest.starts_with('#') && rest != "," {
            return Err(ConfigError {
                line: Some(line_no),
                reason: format!("字符串后面有多余内容「{rest}」"),
            });
        }
        return Ok(Val::Str(out));
    }

    // 数字 / 布尔 / 日期：先把注释剥掉再校验
    let token = strip_comment(s);
    let token = token.trim();
    if token == "true" || token == "false" {
        return Err(ConfigError {
            line: Some(line_no),
            reason: "本配置没有布尔键；不支持该语法".into(),
        });
    }
    let is_float = token.contains('.') || token.contains('e') || token.contains('E');
    if is_float {
        let v: f64 = token.parse().map_err(|_| ConfigError {
            line: Some(line_no),
            reason: format!("不是合法数字「{token}」"),
        })?;
        return Ok(Val::Float(v));
    }
    let v: i64 = token.parse().map_err(|_| ConfigError {
        line: Some(line_no),
        reason: format!("不支持的值「{token}」（子集只支持字符串 / 数字 / 字符串数组）"),
    })?;
    Ok(Val::Int(v))
}

/// 去掉引号外的 `#` 注释（数字 / 数组行用）
fn strip_comment(s: &str) -> &str {
    let mut in_str: Option<char> = None;
    for (idx, c) in s.char_indices() {
        match in_str {
            Some(q) => {
                if c == q {
                    in_str = None;
                }
            }
            None => {
                if c == '#' {
                    return &s[..idx];
                }
                if c == '"' || c == '\'' {
                    in_str = Some(c);
                }
            }
        }
    }
    s
}

/// 按逗号切数组元素（引号内的逗号不切）
fn split_array_items(inner: &str, line_no: u32) -> Result<Vec<String>, ConfigError> {
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut in_str: Option<char> = None;
    let mut escaped = false;
    for c in inner.chars() {
        if let Some(q) = in_str {
            cur.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' && q == '"' {
                escaped = true;
            } else if c == q {
                in_str = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                in_str = Some(c);
                cur.push(c);
            }
            ',' => {
                items.push(cur.clone());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if in_str.is_some() {
        return Err(ConfigError {
            line: Some(line_no),
            reason: "数组里的字符串没有闭合".into(),
        });
    }
    items.push(cur);
    Ok(items)
}

/// 把解析出的值落到配置上：只认五个键；越界 → 键级回落 + note。
fn apply(
    cfg: &mut ProjectConfig,
    notes: &mut Vec<ConfigNote>,
    sec: &str,
    key: &str,
    val: Val,
    line_no: u32,
) -> Result<(), ConfigError> {
    let note = |kind: ConfigNoteKind, detail: Option<String>| ConfigNote {
        kind,
        file: String::new(),
        line: Some(line_no),
        detail,
    };
    let full = format!("{sec}.{key}");
    match (sec, key) {
        ("ignore", "projects") | ("ignore", "paths") => {
            let Val::Arr(items) = val else {
                return Err(ConfigError {
                    line: Some(line_no),
                    reason: format!("「{full}」的值必须是字符串数组"),
                });
            };
            let mut out: Vec<String> = Vec::new();
            for it in items {
                let Val::Str(s) = it else {
                    return Err(ConfigError {
                        line: Some(line_no),
                        reason: format!("「{full}」的元素必须是带引号的路径字符串"),
                    });
                };
                // 形式校验：绝对路径或 ~ 开头（§6.1）。不合格的条目跳过 + note，
                // 不整键回落——一个笔误废掉五十条 ignore 太重了
                if !s.starts_with('/') && !s.starts_with('~') {
                    notes.push(note(ConfigNoteKind::Path, Some(s)));
                    continue;
                }
                if out.len() >= IGNORE_MAX {
                    notes.push(note(
                        ConfigNoteKind::Value,
                        Some(format!("{full} 超过 {IGNORE_MAX} 条上限，余项丢弃")),
                    ));
                    break;
                }
                out.push(s);
            }
            if key == "projects" {
                cfg.ignore_projects = out;
            } else {
                cfg.ignore_paths = out;
            }
        }
        ("thresholds", "deps_warn_gb") => {
            let f = match val {
                Val::Float(f) => f,
                Val::Int(i) => i as f64,
                _ => {
                    return Err(ConfigError {
                        line: Some(line_no),
                        reason: format!("「{full}」的值必须是数字"),
                    })
                }
            };
            if !(DEPS_WARN_GB_MIN..=DEPS_WARN_GB_MAX).contains(&f) {
                notes.push(note(
                    ConfigNoteKind::Value,
                    Some(format!(
                        "{full}={f} 超出 {DEPS_WARN_GB_MIN}–{DEPS_WARN_GB_MAX}，已回落默认"
                    )),
                ));
            } else {
                cfg.deps_warn_gb = Some(f);
            }
        }
        ("thresholds", "stale_days") => {
            let Val::Int(d) = val else {
                return Err(ConfigError {
                    line: Some(line_no),
                    reason: format!("「{full}」的值必须是整数"),
                });
            };
            if !(STALE_DAYS_MIN as i64..=STALE_DAYS_MAX as i64).contains(&d) {
                notes.push(note(
                    ConfigNoteKind::Value,
                    Some(format!(
                        "{full}={d} 超出 {STALE_DAYS_MIN}–{STALE_DAYS_MAX}，已回落默认"
                    )),
                ));
            } else {
                cfg.stale_days = Some(d as u64);
            }
        }
        ("watch", "ports") => {
            let Val::Arr(items) = val else {
                return Err(ConfigError {
                    line: Some(line_no),
                    reason: format!("「{full}」的值必须是整数数组"),
                });
            };
            let mut out: Vec<u16> = Vec::new();
            let mut bad = false;
            for it in items {
                let p = match it {
                    Val::Int(i) => i,
                    _ => {
                        return Err(ConfigError {
                            line: Some(line_no),
                            reason: format!("「{full}」的元素必须是端口号（整数）"),
                        })
                    }
                };
                if !(1..=65535).contains(&p) || out.len() >= WATCH_PORTS_MAX {
                    bad = true;
                    break;
                }
                if !out.contains(&(p as u16)) {
                    out.push(p as u16);
                }
            }
            if bad {
                notes.push(note(
                    ConfigNoteKind::Value,
                    Some(format!(
                        "{full} 含越界端口或超过 {WATCH_PORTS_MAX} 个，已整键回落默认"
                    )),
                ));
            } else {
                cfg.watch_ports = out;
            }
        }
        _ => {
            return Err(ConfigError {
                line: Some(line_no),
                reason: format!("不支持的键「{full}」"),
            })
        }
    }
    Ok(())
}

// -------------------------------------------------------------- 一轮扫描的汇总

/// 一轮扫描收集到的全部项目配置（只读项目根那一份，子包配置不生效）。
#[derive(Debug, Default)]
pub struct ProjectConfigs {
    pub by_root: BTreeMap<String, ProjectConfig>,
    pub notes: Vec<ConfigNote>,
    /// 被 `ignore.projects` 跳过的项目根（绝对路径字符串）
    pub ignored_roots: Vec<String>,
}

impl ProjectConfigs {
    /// 对每个项目根读一份 `prune.toml`。roots 必须已是去重后的项目根。
    pub fn collect<I>(roots: I, home: &Path) -> Self
    where
        I: IntoIterator,
        I::Item: AsRef<Path>,
    {
        let mut out = Self::default();
        for root in roots {
            let root = root.as_ref();
            let root_str = root.to_string_lossy().to_string();
            let path = root.join(FILE_NAME);
            let parsed = match load_at(&path) {
                Ok(Some(p)) => p,
                Ok(None) => continue,
                Err(e) => {
                    // 整份回落默认 + 明确提示（§6.3）
                    out.notes.push(ConfigNote {
                        kind: ConfigNoteKind::Syntax,
                        file: path.to_string_lossy().to_string(),
                        line: e.line,
                        detail: Some(e.reason),
                    });
                    continue;
                }
            };
            // ignore.projects 命中判定：root 等于条目或在其之下（展开 ~ 后按字符串前缀比）
            for raw in &parsed.config.ignore_projects {
                let target = expand_path(raw, home);
                let t = target.to_string_lossy().to_string();
                let hit = root_str == t
                    || (root_str.starts_with(&t) && root_str[t.len()..].starts_with('/'));
                if hit {
                    out.ignored_roots.push(root_str.clone());
                    out.notes.push(ConfigNote {
                        kind: ConfigNoteKind::Ignored,
                        file: path.to_string_lossy().to_string(),
                        line: None,
                        detail: Some(root_str.clone()),
                    });
                    break;
                }
            }
            let cfg = parsed.config;
            let mut notes = parsed.notes;
            notes
                .iter_mut()
                .for_each(|n| n.file = path.to_string_lossy().to_string());
            out.notes.extend(notes);
            out.by_root.insert(root_str, cfg);
        }
        out
    }

    /// 项目是否被用户配置跳过（不扫、不进账本、不进体检卡）
    pub fn is_ignored(&self, root: &Path) -> bool {
        let s = root.to_string_lossy();
        self.ignored_roots.iter().any(|r| *r == s)
    }

    /// 某项目的阈值覆盖。没配置的项目返回内置默认。
    pub fn for_root(&self, root: &Path) -> ProjectConfig {
        self.by_root
            .get(root.to_string_lossy().as_ref())
            .cloned()
            .unwrap_or_default()
    }

    /// 所有项目声明的关注端口（并集、去重、升序）。端口采样侧并入关注层。
    pub fn watch_ports(&self) -> Vec<u16> {
        let mut v: Vec<u16> = self
            .by_root
            .values()
            .flat_map(|c| c.watch_ports.iter().copied())
            .collect();
        v.sort_unstable();
        v.dedup();
        v.truncate(WATCH_PORTS_MAX);
        v
    }
}

/// 端口采样侧的轻量入口：只取各项目 `watch.ports` 的并集。
///
/// 与 [`ProjectConfigs::collect`] 分开——端口轮每 30 s 跑一次，不需要
/// ignore 匹配与 note 收集，一个 stat + （命中才）一次小文件读取就够。
pub fn collect_watch_ports<I>(roots: I) -> Vec<u16>
where
    I: IntoIterator,
    I::Item: AsRef<Path>,
{
    let mut out: Vec<u16> = Vec::new();
    for root in roots {
        let Ok(Some(parsed)) = load_at(&root.as_ref().join(FILE_NAME)) else {
            continue;
        };
        for p in parsed.config.watch_ports {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out.sort_unstable();
    out.truncate(WATCH_PORTS_MAX);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(text: &str) -> Parsed {
        parse(text).unwrap_or_else(|e| panic!("应解析成功: {e}"))
    }

    fn parse_err(text: &str) -> ConfigError {
        parse(text).expect_err("应整份拒绝")
    }

    // ---------------------------------------------------------- 基础语法

    #[test]
    fn empty_file_is_zero_config() {
        let p = parse_ok("");
        assert_eq!(p.config, ProjectConfig::default());
        assert!(p.notes.is_empty());
    }

    /// 模板是给用户落盘的起点——它自己必须能被解析器干净吃下，
    /// 否则设置页复制的第一份配置就是报错的（W2/D-09 门禁）。
    #[test]
    fn template_parses_cleanly() {
        let p = parse_ok(TEMPLATE);
        assert_eq!(p.config, ProjectConfig::default());
        assert!(p.notes.is_empty(), "模板不应产生任何说明事件");
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let p = parse_ok("# 注释\n\n   \n[thresholds]\n# 注释\ndeps_warn_gb = 2.0\n");
        assert_eq!(p.config.deps_warn_gb, Some(2.0));
    }

    #[test]
    fn full_example_parses() {
        let p = parse_ok(
            "[ignore]\nprojects = [\"~/work/legacy-api\", \"/tmp/x\"]\npaths = [\"~/Library/Caches/yarn/npm-archive\"]\n\n[thresholds]\ndeps_warn_gb = 2.0\nstale_days = 90\n\n[watch]\nports = [3000, 5173, 8080]\n",
        );
        assert_eq!(
            p.config.ignore_projects,
            vec!["~/work/legacy-api", "/tmp/x"]
        );
        assert_eq!(p.config.stale_days, Some(90));
        assert_eq!(p.config.watch_ports, vec![3000, 5173, 8080]);
        assert!(p.notes.is_empty());
    }

    #[test]
    fn array_can_span_lines_and_allow_trailing_comma() {
        let p = parse_ok("[ignore]\nprojects = [\n  \"~/a\",\n  \"~/b\",\n]\n");
        assert_eq!(p.config.ignore_projects, vec!["~/a", "~/b"]);
    }

    #[test]
    fn literal_and_basic_strings_both_work() {
        let p = parse_ok("[ignore]\nprojects = ['~/a', \"~/b\"]\n");
        assert_eq!(p.config.ignore_projects, vec!["~/a", "~/b"]);
    }

    // ---------------------------------------------------------- 明确拒绝

    #[test]
    fn nested_table_is_rejected_with_line_number() {
        let e = parse_err("[a.b]\nx = 1\n");
        assert_eq!(e.line, Some(1));
        assert!(e.reason.contains("不支持嵌套表"));
    }

    #[test]
    fn boolean_is_rejected() {
        let e = parse_err("[thresholds]\nstale_days = true\n");
        assert!(e.reason.contains("不支持该语法"), "{}", e.reason);
    }

    #[test]
    fn unknown_key_is_rejected() {
        let e = parse_err("[thresholds]\nfoo = 1\n");
        assert!(e.reason.contains("不支持的键"), "{}", e.reason);
    }

    #[test]
    fn duplicate_key_is_rejected() {
        let e = parse_err("[thresholds]\nstale_days = 30\nstale_days = 40\n");
        assert!(e.reason.contains("重复定义"), "{}", e.reason);
    }

    #[test]
    fn duplicate_section_is_rejected() {
        let e = parse_err("[ignore]\npaths = []\n[ignore]\npaths = []\n");
        assert!(e.reason.contains("重复定义"), "{}", e.reason);
    }

    #[test]
    fn key_without_section_is_rejected() {
        let e = parse_err("stale_days = 30\n");
        assert!(e.reason.contains("[section]"), "{}", e.reason);
    }

    #[test]
    fn malformed_line_reports_line_number() {
        let e = parse_err("[thresholds]\n\n这不是一行配置\n");
        assert_eq!(e.line, Some(3));
    }

    // ---------------------------------------------------------- 安全边界

    #[test]
    fn reclaim_section_is_banned_whole_file() {
        let e = parse_err("[reclaim]\nauto = true\n");
        assert!(e.reason.contains("自动执行"), "{}", e.reason);
    }

    #[test]
    fn clean_and_auto_sections_are_banned() {
        for sec in ["clean", "auto"] {
            let e = parse_err(&format!("[{sec}]\nx = 1\n"));
            assert!(e.reason.contains("自动执行"), "{sec}: {}", e.reason);
        }
    }

    #[test]
    fn extra_paths_key_is_banned_even_inside_allowed_section() {
        let e = parse_err("[ignore]\nextra_paths = [\"~/x\"]\n");
        assert!(e.reason.contains("只能减少范围"), "{}", e.reason);
    }

    #[test]
    fn root_and_home_keys_are_banned() {
        for k in ["root", "home"] {
            let e = parse_err(&format!("[thresholds]\n{k} = \"/x\"\n"));
            assert!(e.reason.contains("路径重定向"), "{k}: {}", e.reason);
        }
    }

    // ---------------------------------------------------------- 值级回落

    #[test]
    fn out_of_range_threshold_falls_back_with_note() {
        let p = parse_ok("[thresholds]\ndeps_warn_gb = 500.0\nstale_days = 3\n");
        assert_eq!(p.config.deps_warn_gb, None, "越界键回落默认");
        assert_eq!(p.config.stale_days, None);
        assert_eq!(p.notes.len(), 2);
        assert!(p.notes.iter().all(|n| n.kind == ConfigNoteKind::Value));
        assert!(p
            .notes
            .iter()
            .any(|n| n.detail.as_deref().unwrap_or("").contains("已回落默认")));
    }

    #[test]
    fn other_keys_survive_one_out_of_range_key() {
        let p = parse_ok("[thresholds]\ndeps_warn_gb = 500.0\n\n[watch]\nports = [3000]\n");
        assert_eq!(p.config.deps_warn_gb, None, "越界键回落");
        assert_eq!(p.config.watch_ports, vec![3000], "其余配置不受牵连");
    }

    #[test]
    fn path_entry_without_absolute_or_tilde_is_dropped_with_note() {
        let p = parse_ok("[ignore]\nprojects = [\"relative/x\", \"~/ok\"]\n");
        assert_eq!(p.config.ignore_projects, vec!["~/ok"]);
        assert_eq!(p.notes.len(), 1);
        assert_eq!(p.notes[0].kind, ConfigNoteKind::Path);
        assert_eq!(p.notes[0].detail.as_deref(), Some("relative/x"));
    }

    #[test]
    fn watch_ports_out_of_range_falls_back_whole_key() {
        let p = parse_ok("[watch]\nports = [3000, 0, 5173]\n");
        assert!(p.config.watch_ports.is_empty(), "含 0 端口整键回落");
        assert_eq!(p.notes.len(), 1);
    }

    #[test]
    fn watch_ports_dedup_within_cap() {
        let p = parse_ok("[watch]\nports = [3000, 3000, 5173]\n");
        assert_eq!(p.config.watch_ports, vec![3000, 5173]);
    }

    #[test]
    fn heterogeneous_array_is_rejected() {
        let e = parse_err("[watch]\nports = [3000, \"x\"]\n");
        assert!(e.reason.contains("类型不一致"), "{}", e.reason);
    }

    // ---------------------------------------------------------- 汇总与路径

    #[test]
    fn expand_path_handles_tilde_forms() {
        let home = PathBuf::from("/Users/x");
        assert_eq!(expand_path("~", &home), home);
        assert_eq!(
            expand_path("~/work/a", &home),
            PathBuf::from("/Users/x/work/a")
        );
        assert_eq!(expand_path("/tmp/x", &home), PathBuf::from("/tmp/x"));
    }

    #[test]
    fn collect_reports_syntax_fallback_and_keeps_scanning() {
        let dir = std::env::temp_dir().join(format!("prune-toml-{}", std::process::id()));
        let root = dir.join("broken");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(FILE_NAME), "[reclaim]\nauto = true\n").unwrap();
        let clean = dir.join("clean");
        std::fs::create_dir_all(&clean).unwrap();

        let cs = ProjectConfigs::collect([&root, &clean], &dir);
        assert!(
            !cs.by_root.contains_key(&root.to_string_lossy().to_string()),
            "坏文件整份回落"
        );
        assert_eq!(cs.notes.len(), 1);
        assert_eq!(cs.notes[0].kind, ConfigNoteKind::Syntax);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn collect_marks_ignored_roots_and_emits_ignored_note() {
        let dir = std::env::temp_dir().join(format!("prune-toml-ig-{}", std::process::id()));
        let root = dir.join("legacy-api");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(FILE_NAME),
            "[ignore]\nprojects = [\"~/legacy-api\"]\n",
        )
        .unwrap();

        // home = dir：`~/legacy-api` 展开后即 root
        let cs = ProjectConfigs::collect(std::iter::once(root.as_path()), &dir);
        assert!(cs.is_ignored(&root));
        assert_eq!(cs.ignored_roots, vec![root.to_string_lossy().to_string()]);
        assert!(cs.notes.iter().any(|n| n.kind == ConfigNoteKind::Ignored));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn thresholds_override_reaches_byte_and_day_accessors() {
        let cfg = ProjectConfig {
            deps_warn_gb: Some(2.0),
            stale_days: Some(90),
            ..Default::default()
        };
        assert_eq!(cfg.heavy_bytes(), 2 * 1024 * 1024 * 1024);
        assert_eq!(cfg.stale_days_or_default(), 90);
        // 默认值与 disk::project 常量一致（不另立一套）
        assert_eq!(
            ProjectConfig::default().heavy_bytes(),
            crate::disk::project::PROJECT_HEAVY_BYTES
        );
        assert_eq!(
            ProjectConfig::default().stale_days_or_default(),
            crate::disk::project::PROJECT_STALE_DAYS
        );
    }

    /// 端口采样侧轻量入口：只读 watch.ports，坏文件静默跳过（端口轮不产生 note）
    #[test]
    fn collect_watch_ports_reads_only_that_key_and_survives_broken_files() {
        let dir = std::env::temp_dir().join(format!("prune-toml-wp-{}", std::process::id()));
        let a = dir.join("a");
        let b = dir.join("b");
        let c = dir.join("c");
        for d in [&a, &b, &c] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(a.join(FILE_NAME), "[watch]\nports = [3000, 5173]\n").unwrap();
        std::fs::write(b.join(FILE_NAME), "[watch]\nports = [5173, 8080]\n").unwrap();
        std::fs::write(c.join(FILE_NAME), "[reclaim]\nauto = true\n").unwrap();

        let got = collect_watch_ports([&a, &b, &c]);
        assert_eq!(
            got,
            vec![3000, 5173, 8080],
            "并集去重升序，坏文件不拖垮整轮"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn watch_ports_union_is_deduped_and_capped() {
        let mut cs = ProjectConfigs::default();
        let a = ProjectConfig {
            watch_ports: vec![3000, 5173],
            ..Default::default()
        };
        let b = ProjectConfig {
            watch_ports: vec![5173, 8080],
            ..Default::default()
        };
        cs.by_root.insert("/a".into(), a);
        cs.by_root.insert("/b".into(), b);
        assert_eq!(cs.watch_ports(), vec![3000, 5173, 8080]);
    }
}
