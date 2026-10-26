use prune_core::mem::process::Scanner;
fn main() {
    let mut s = Scanner::new();
    let procs = s.scan();
    // 最大的几个 renderer 的完整 argv
    let mut r: Vec<_> = procs.iter().filter(|p| p.name.contains("Renderer")).collect();
    r.sort_by_key(|p| std::cmp::Reverse(p.rss));
    for p in r.iter().take(3) {
        println!("== pid {} {} MB ==", p.pid, p.rss/1024/1024);
        for a in &p.cmd {
            if a.contains("workspace") || a.contains("folder") || a.contains("Users") || a.contains("vscode-") {
                println!("  {}", &a[..a.len().min(160)]);
            }
        }
    }
    // exthost 的标题形态
    let mut e: Vec<_> = procs.iter().filter(|p| p.cmd.first().map(|c| c.contains("extension-host")).unwrap_or(false)).collect();
    e.sort_by_key(|p| std::cmp::Reverse(p.rss));
    println!("\n== extension host 标题 ==");
    for p in e.iter().take(6) {
        println!("  pid {} {} MB · {:?}", p.pid, p.rss/1024/1024, p.cmd.first());
    }
    // lsof 是否可用（受沙箱影响）
    println!("\n== lsof 探测 ==");
    for p in r.iter().take(2) {
        let paths = prune_core::mem::workspace::open_file_paths(p.pid);
        println!("  pid {} → {} 条路径，样例 {:?}", p.pid, paths.len(), paths.iter().filter(|x| x.contains("/Users/")).take(2).collect::<Vec<_>>());
    }
}
