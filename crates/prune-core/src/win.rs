//! Windows 专属系统调用封装（仅 Windows 编译）。
//!
//! 为什么用 `windows-sys` 而不是手写 `extern "system"`：
//! `PROCESS_MEMORY_COUNTERS` 有 10 个字段，`MEMORYSTATUSEX` 有 7 个，
//! 手写 struct 一旦布局/对齐错位，读出来的是垃圾数值——而这种错误在 macOS 上
//! 永远测不出来（编译能过、类型也对）。用官方绑定换掉这份风险。
//!
//! 口径说明：这里只做「读」，不做任何「释放内存」动作。

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

/// `GlobalMemoryStatusEx` 的结果（物理内存视角）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SysMemory {
    pub total: u64,
    /// `ullAvailPhys`：可用物理内存（已排除 standby/cached，可直接分配）
    pub available: u64,
    /// `dwMemoryLoad`：系统给出的物理内存占用百分比（0–100）
    pub load_pct: u32,
}

/// 读物理内存总量 / 可用量 / 占用率。
pub fn system_memory() -> Option<SysMemory> {
    // SAFETY: dwLength 必须由调用方填成结构体大小，否则 API 直接返回失败
    let mut st: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    st.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    let ok = unsafe { GlobalMemoryStatusEx(&mut st) };
    (ok != 0).then_some(SysMemory {
        total: st.ullTotalPhys,
        available: st.ullAvailPhys,
        load_pct: st.dwMemoryLoad,
    })
}

/// 进程私有提交字节（Private Bytes），对应任务管理器「提交大小」。
///
/// API 里的字段名是 `PagefileUsage`，语义就是 Private Bytes（提交内存）：
/// 含堆、栈、私有映射页。**不含已被换出到 pagefile 的部分**——Windows 没有
/// 等价的可读口径，因此 Windows 上 `swapped` 一律为 `None`，UI 隐藏换出读数。
pub fn private_bytes(pid: i32) -> Option<u64> {
    if pid <= 0 {
        return None;
    }
    // SAFETY: OpenProcess 失败返回空句柄；成功时下面一定 CloseHandle
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid as u32) };
    if handle.is_null() {
        return None;
    }

    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: 句柄有效、结构体在栈上且 cb 已填
    let ok = unsafe { K32GetProcessMemoryInfo(handle, &mut counters, counters.cb) };
    // SAFETY: 句柄由本函数独占持有，关闭一次
    unsafe { CloseHandle(handle) };

    (ok != 0).then_some(counters.PagefileUsage as u64)
}
