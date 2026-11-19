//! SMC 非 macOS 空实现：保证命令注册与模块路径两端一致（island 同款模式）。

/// 非 macOS 永远读不到 SMC。
pub fn cpu_temp_median() -> Option<f64> {
    None
}
