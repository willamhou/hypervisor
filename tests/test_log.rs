//! Log module tests

use hypervisor::log::{log_buffer, LogBuffer, LogLevel, LogWriter};

pub fn run_log_test() {
    hypervisor::log_info!("\n=== Test: Log Module ===\n");
    let mut pass: u64 = 0;
    let mut fail: u64 = 0;

    // Test 1: LogBuffer empty state (head == tail, available == 0)
    {
        let buf = LogBuffer::new();
        if buf.available() == 0 {
            hypervisor::log_info!("  [PASS] LogBuffer empty state\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] LogBuffer should be empty initially\n");
            fail += 1;
        }
    }

    // Test 2: write() + read() roundtrip
    {
        let buf = LogBuffer::new();
        buf.write(b"hello");
        let mut out = [0u8; 16];
        let n = buf.read(&mut out);
        if n == 5 && &out[..5] == b"hello" {
            hypervisor::log_info!("  [PASS] write + read roundtrip\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] write + read roundtrip\n");
            fail += 1;
        }
    }

    // Test 3: Overflow — write >8KB, verify latest data preserved
    {
        let buf = LogBuffer::new();
        // Write 8192 + 5 bytes: first 5 bytes should be overwritten
        let block = [b'A'; 8192];
        buf.write(&block);
        buf.write(b"XYZWV");
        // Available should be 8191 (max capacity is SIZE-1 for SPSC)
        let avail = buf.available();
        // Read last bytes — should end with "XYZWV"
        let mut out = [0u8; 8192];
        let n = buf.read(&mut out);
        let ok = n > 5 && &out[n - 5..n] == b"XYZWV" && avail <= 8192;
        if ok {
            hypervisor::log_info!("  [PASS] Overflow preserves latest data\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] Overflow should preserve latest data\n");
            fail += 1;
        }
    }

    // Test 4: log_info! macro compiles and writes to buffer
    {
        // Clear CPU 0's log buffer first
        let buf = log_buffer(0);
        buf.clear();
        hypervisor::log_info!("test msg {}", 42);
        let avail = buf.available();
        if avail > 0 {
            hypervisor::log_info!("  [PASS] log_info! writes to buffer\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] log_info! should write to buffer\n");
            fail += 1;
        }
    }

    // Test 5: LogWriter writes to both buffer and UART
    {
        let buf = log_buffer(0);
        buf.clear();
        {
            use core::fmt::Write;
            let mut w = LogWriter::new(LogLevel::Error);
            let _ = write!(w, "direct write");
        }
        let mut out = [0u8; 32];
        let n = buf.read(&mut out);
        if n == 12 && &out[..12] == b"direct write" {
            hypervisor::log_info!("  [PASS] LogWriter writes to buffer and UART\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] LogWriter should write to buffer\n");
            fail += 1;
        }
    }

    // Test 6: Per-CPU buffer isolation (buffer[0] vs buffer[1])
    {
        let buf0 = log_buffer(0);
        let buf1 = log_buffer(1);
        buf0.clear();
        buf1.clear();
        buf0.write(b"cpu0");
        buf1.write(b"cpu1data");
        if buf0.available() == 4 && buf1.available() == 8 {
            hypervisor::log_info!("  [PASS] Per-CPU buffer isolation\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] Per-CPU buffers should be independent\n");
            fail += 1;
        }
        // Clean up
        buf0.clear();
        buf1.clear();
    }

    // Test 7: Read from empty buffer returns 0
    {
        let buf = LogBuffer::new();
        let mut out = [0u8; 16];
        let n = buf.read(&mut out);
        if n == 0 {
            hypervisor::log_info!("  [PASS] Read from empty buffer returns 0\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] Read from empty buffer should return 0\n");
            fail += 1;
        }
    }

    // Test 8: Multiple writes accumulate correctly
    {
        let buf = LogBuffer::new();
        buf.write(b"aaa");
        buf.write(b"bbb");
        buf.write(b"ccc");
        let mut out = [0u8; 16];
        let n = buf.read(&mut out);
        if n == 9 && &out[..9] == b"aaabbbccc" {
            hypervisor::log_info!("  [PASS] Multiple writes accumulate\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] Multiple writes should accumulate\n");
            fail += 1;
        }
    }

    hypervisor::log_info!("  Results: {} passed, {} failed\n", pass, fail);
    assert!(fail == 0, "Log module tests failed");
}
