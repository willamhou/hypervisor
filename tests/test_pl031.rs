//! PL031 RTC emulation tests

use hypervisor::devices::pl031::VirtualPl031;
use hypervisor::devices::MmioDevice;

pub fn run_pl031_test() {
    hypervisor::log_info!("\n=== Test: PL031 RTC Emulation ===\n");
    let mut pass: u64 = 0;
    let mut fail: u64 = 0;

    let mut rtc = VirtualPl031::new();

    // Test 1: RTCDR is readable (returns Some, value >= 0 is valid at boot)
    {
        let val = rtc.read(0x000, 4);
        if val.is_some() {
            hypervisor::log_info!("  [PASS] RTCDR readable\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] RTCDR should be readable\n");
            fail += 1;
        }
    }

    // Test 2: Write RTCLR, read back via RTCDR
    {
        rtc.write(0x008, 1000, 4);
        rtc.write(0x00C, 1, 4); // enable
        let val = rtc.read(0x000, 4).unwrap();
        if val >= 1000 {
            hypervisor::log_info!("  [PASS] RTCLR write + RTCDR readback\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] RTCLR write + RTCDR readback\n");
            fail += 1;
        }
    }

    // Test 3: PeriphID registers match PL031
    {
        let id0 = rtc.read(0xFE0, 4).unwrap();
        let id1 = rtc.read(0xFE4, 4).unwrap();
        let id2 = rtc.read(0xFE8, 4).unwrap();
        let pcell0 = rtc.read(0xFF0, 4).unwrap();
        let pcell1 = rtc.read(0xFF4, 4).unwrap();
        if id0 == 0x31 && id1 == 0x10 && id2 == 0x04 && pcell0 == 0x0D && pcell1 == 0xF0 {
            hypervisor::log_info!("  [PASS] PeriphID/PrimeCellID correct\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] PeriphID/PrimeCellID mismatch\n");
            fail += 1;
        }
    }

    // Test 4: Unknown offset returns 0
    {
        let val = rtc.read(0x100, 4).unwrap();
        if val == 0 {
            hypervisor::log_info!("  [PASS] Unknown offset returns 0\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] Unknown offset should return 0\n");
            fail += 1;
        }
    }

    hypervisor::log_info!("  Results: {} passed, {} failed\n", pass, fail);
    assert!(fail == 0, "PL031 RTC tests failed");
}
