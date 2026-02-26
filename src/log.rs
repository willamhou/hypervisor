//! Hypervisor Logging Module
//!
//! Provides a unified logging API with an in-memory ring buffer and UART output.
//!
//! - **LogBuffer**: 8KB per-CPU static ring buffer (atomic head/tail, overwrites oldest)
//! - **LogLevel**: Error/Warn/Info/Debug with compile-time and runtime gating
//! - **LogWriter**: `core::fmt::Write` impl that writes to both buffer and UART
//! - **Macros**: `log_error!`, `log_warn!`, `log_info!`, `log_debug!`
//!
//! The panic handler and assembly `fault_diag_print` continue to use raw `uart_puts()`
//! directly to avoid recursion and work without Rust runtime.

use core::cell::UnsafeCell;
use core::fmt;
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use crate::platform::MAX_SMP_CPUS;

// ── Log levels ──────────────────────────────────────────────────────

/// Log severity levels (lower = more severe).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum LogLevel {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
}

/// Compile-time maximum log level. Messages above this are compiled out entirely.
#[cfg(debug_assertions)]
pub const MAX_LOG_LEVEL: LogLevel = LogLevel::Debug;
#[cfg(not(debug_assertions))]
pub const MAX_LOG_LEVEL: LogLevel = LogLevel::Info;

/// Runtime UART log level. Messages above this are buffered but not printed.
/// Can be changed at runtime via `set_uart_log_level()`.
static UART_LOG_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Debug as u8);

/// Set the runtime UART output log level.
pub fn set_uart_log_level(level: LogLevel) {
    UART_LOG_LEVEL.store(level as u8, Ordering::Relaxed);
}

/// Get the current runtime UART output log level.
pub fn uart_log_level() -> u8 {
    UART_LOG_LEVEL.load(Ordering::Relaxed)
}

// ── Log buffer ──────────────────────────────────────────────────────

/// Per-CPU ring buffer size in bytes.
const LOG_BUF_SIZE: usize = 8192; // 8KB

/// Lock-free ring buffer for log messages.
///
/// Follows the `UartRxRing` pattern from `src/global.rs`: atomic head/tail,
/// `UnsafeCell` for the data array. Single-writer (logging on this CPU),
/// single-reader (debugger/dump). Overwrites oldest data on overflow.
pub struct LogBuffer {
    buf: UnsafeCell<[u8; LOG_BUF_SIZE]>,
    head: AtomicUsize, // read index (oldest unread byte)
    tail: AtomicUsize, // write index (next byte to write)
}

// Safety: LogBuffer uses atomic indices for synchronization.
// Only one CPU writes to each per-CPU buffer (single producer).
unsafe impl Sync for LogBuffer {}

impl Default for LogBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl LogBuffer {
    pub const fn new() -> Self {
        Self {
            buf: UnsafeCell::new([0; LOG_BUF_SIZE]),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Write bytes into the ring buffer. Overwrites oldest data if full.
    pub fn write(&self, data: &[u8]) {
        let buf = unsafe { &mut *self.buf.get() };
        let mut tail = self.tail.load(Ordering::Relaxed);

        for &byte in data {
            buf[tail] = byte;
            tail = (tail + 1) % LOG_BUF_SIZE;

            // If tail catches head, advance head (overwrite oldest)
            let head = self.head.load(Ordering::Acquire);
            if tail == head {
                self.head
                    .store((head + 1) % LOG_BUF_SIZE, Ordering::Release);
            }
        }

        self.tail.store(tail, Ordering::Release);
    }

    /// Read available bytes into `out`. Returns number of bytes read.
    pub fn read(&self, out: &mut [u8]) -> usize {
        let mut head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        let buf = unsafe { &*self.buf.get() };

        let mut count = 0;
        while head != tail && count < out.len() {
            out[count] = buf[head];
            head = (head + 1) % LOG_BUF_SIZE;
            count += 1;
        }

        self.head.store(head, Ordering::Release);
        count
    }

    /// Return the number of bytes available to read.
    pub fn available(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        if tail >= head {
            tail - head
        } else {
            LOG_BUF_SIZE - head + tail
        }
    }

    /// Reset the buffer to empty state (for testing).
    pub fn clear(&self) {
        self.head.store(0, Ordering::Release);
        self.tail.store(0, Ordering::Release);
    }
}

// ── Per-CPU buffer array ────────────────────────────────────────────

/// Per-CPU log buffers. Indexed by CPU ID (MPIDR_EL1.Aff0 in multi-pCPU mode).
#[allow(clippy::declare_interior_mutable_const)]
static LOG_BUFFERS: [LogBuffer; MAX_SMP_CPUS] = {
    const EMPTY: LogBuffer = LogBuffer::new();
    [EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY]
};

/// Get the log buffer for the current CPU.
///
/// In multi-pCPU mode, reads MPIDR_EL1.Aff0 for the CPU index.
/// Otherwise returns buffer 0 (single-pCPU / unit test mode).
pub fn current_log_buffer() -> &'static LogBuffer {
    let cpu_id = current_cpu_id();
    &LOG_BUFFERS[cpu_id]
}

/// Get a specific CPU's log buffer (for testing / cross-CPU reads).
pub fn log_buffer(cpu_id: usize) -> &'static LogBuffer {
    &LOG_BUFFERS[cpu_id]
}

/// Read the current CPU ID from MPIDR_EL1.Aff0.
#[inline]
fn current_cpu_id() -> usize {
    #[cfg(feature = "multi_pcpu")]
    {
        let mpidr: u64;
        unsafe {
            core::arch::asm!("mrs {}, mpidr_el1", out(reg) mpidr, options(nostack, nomem));
        }
        (mpidr & 0xFF) as usize
    }
    #[cfg(not(feature = "multi_pcpu"))]
    {
        0
    }
}

// ── Log writer ──────────────────────────────────────────────────────

/// Writer that routes formatted output to both LogBuffer and UART.
///
/// Implements `core::fmt::Write` so it works with `write!()` / `writeln!()`.
/// UART output is gated by `UART_LOG_LEVEL` — buffer always receives data.
pub struct LogWriter {
    level: LogLevel,
}

impl LogWriter {
    pub const fn new(level: LogLevel) -> Self {
        Self { level }
    }
}

impl fmt::Write for LogWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // Always write to the per-CPU log buffer
        current_log_buffer().write(s.as_bytes());

        // Conditionally write to UART based on runtime level
        if (self.level as u8) <= UART_LOG_LEVEL.load(Ordering::Relaxed) {
            crate::uart_puts(s.as_bytes());
        }

        Ok(())
    }
}

// ── Initialization ──────────────────────────────────────────────────

static LOG_INITIALIZED: AtomicU8 = AtomicU8::new(0);

/// Initialize the log module. Called from `rust_main()` and `rust_main_sel2()`.
///
/// Currently a no-op beyond setting the initialized flag (all state is
/// statically initialized). Future: configurable levels from DTB/manifest.
pub fn init() {
    LOG_INITIALIZED.store(1, Ordering::Release);
}

/// Check if the log module has been initialized.
pub fn is_initialized() -> bool {
    LOG_INITIALIZED.load(Ordering::Acquire) != 0
}

// ── Macros ──────────────────────────────────────────────────────────

/// Log at Error level. Always compiled in.
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let _ = write!($crate::log::LogWriter::new($crate::log::LogLevel::Error), $($arg)*);
    }};
}

/// Log at Warn level. Compiled out in release if MAX_LOG_LEVEL < Warn.
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {{
        if ($crate::log::LogLevel::Warn as u8) <= ($crate::log::MAX_LOG_LEVEL as u8) {
            use core::fmt::Write;
            let _ = write!($crate::log::LogWriter::new($crate::log::LogLevel::Warn), $($arg)*);
        }
    }};
}

/// Log at Info level. Compiled out in release if MAX_LOG_LEVEL < Info.
#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {{
        if ($crate::log::LogLevel::Info as u8) <= ($crate::log::MAX_LOG_LEVEL as u8) {
            use core::fmt::Write;
            let _ = write!($crate::log::LogWriter::new($crate::log::LogLevel::Info), $($arg)*);
        }
    }};
}

/// Log at Debug level. Compiled out in release builds.
#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {{
        if ($crate::log::LogLevel::Debug as u8) <= ($crate::log::MAX_LOG_LEVEL as u8) {
            use core::fmt::Write;
            let _ = write!($crate::log::LogWriter::new($crate::log::LogLevel::Debug), $($arg)*);
        }
    }};
}

/// Print macro (without newline) — routes through log_info! level.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let _ = write!($crate::log::LogWriter::new($crate::log::LogLevel::Info), $($arg)*);
    }};
}

/// Println macro (with newline) — routes through log_info! level.
#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n")
    };
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let _ = writeln!($crate::log::LogWriter::new($crate::log::LogLevel::Info), $($arg)*);
    }};
}
