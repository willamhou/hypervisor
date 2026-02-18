//! VSwitch L2 forwarding tests

use hypervisor::vswitch::{PORT_RX, MAX_FRAME_SIZE};
use hypervisor::uart_puts;

pub fn run_vswitch_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  VSwitch Test\n");
    uart_puts(b"========================================\n\n");

    // Setup: register 2 ports
    hypervisor::vswitch::vswitch_reset();
    hypervisor::vswitch::vswitch_add_port(0);
    hypervisor::vswitch::vswitch_add_port(1);

    // Drain any leftover frames from previous tests
    let mut drain_buf = [0u8; MAX_FRAME_SIZE];
    while PORT_RX[0].take(&mut drain_buf).is_some() {}
    while PORT_RX[1].take(&mut drain_buf).is_some() {}

    // Build test Ethernet frames
    // Frame from port 0: src=AA:BB:CC:DD:EE:00, dst=11:22:33:44:55:66
    let mut frame0 = [0u8; 64];
    // dst MAC
    frame0[0..6].copy_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
    // src MAC
    frame0[6..12].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x00]);

    // Test 1: Unknown unicast -> flood all ports except src
    uart_puts(b"[VSWITCH] Test 1: Unknown unicast flood...\n");
    hypervisor::vswitch::vswitch_forward(0, &frame0);
    let mut buf = [0u8; MAX_FRAME_SIZE];
    let len = PORT_RX[1].take(&mut buf);
    assert_ok(len.is_some(), "port 1 should receive flooded frame");
    let len0 = PORT_RX[0].take(&mut buf);
    assert_ok(len0.is_none(), "port 0 (src) should NOT receive own frame");
    uart_puts(b"[VSWITCH] Test 1 PASSED\n\n");

    // Test 2: MAC learning — src MAC AA:BB:CC:DD:EE:00 learned on port 0
    // Now send frame FROM port 1 TO that learned MAC
    uart_puts(b"[VSWITCH] Test 2: MAC learning + precise forward...\n");
    let mut frame1 = [0u8; 64];
    // dst = previously learned MAC (AA:BB:CC:DD:EE:00 -> port 0)
    frame1[0..6].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x00]);
    // src = port 1's MAC
    frame1[6..12].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x01]);
    hypervisor::vswitch::vswitch_forward(1, &frame1);
    let len = PORT_RX[0].take(&mut buf);
    assert_ok(len.is_some(), "port 0 should receive precisely forwarded frame");
    uart_puts(b"[VSWITCH] Test 2 PASSED\n\n");

    // Test 3: Broadcast floods all ports except src
    uart_puts(b"[VSWITCH] Test 3: Broadcast flood...\n");
    let mut bcast = [0u8; 64];
    bcast[0..6].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    bcast[6..12].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x00]);
    hypervisor::vswitch::vswitch_forward(0, &bcast);
    let len = PORT_RX[1].take(&mut buf);
    assert_ok(len.is_some(), "port 1 should receive broadcast");
    let len = PORT_RX[0].take(&mut buf);
    assert_ok(len.is_none(), "port 0 (src) should NOT receive own broadcast");
    uart_puts(b"[VSWITCH] Test 3 PASSED\n\n");

    // Test 4: No self-delivery on unicast
    uart_puts(b"[VSWITCH] Test 4: No self-delivery...\n");
    // Send from port 0 to port 0's own learned MAC
    let mut self_frame = [0u8; 64];
    self_frame[0..6].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x00]); // dst = port 0
    self_frame[6..12].copy_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]); // src
    hypervisor::vswitch::vswitch_forward(0, &self_frame);
    // Port 0 should NOT get its own frame (dst_port == src_port)
    let len = PORT_RX[0].take(&mut buf);
    assert_ok(len.is_none(), "no self-delivery when dst_port == src_port");
    uart_puts(b"[VSWITCH] Test 4 PASSED\n\n");

    // Test 5: MAC table capacity (fill 16 entries)
    uart_puts(b"[VSWITCH] Test 5: MAC table capacity...\n");
    hypervisor::vswitch::vswitch_reset();
    hypervisor::vswitch::vswitch_add_port(0);
    hypervisor::vswitch::vswitch_add_port(1);
    // Drain rings
    while PORT_RX[0].take(&mut drain_buf).is_some() {}
    while PORT_RX[1].take(&mut drain_buf).is_some() {}

    for i in 0..16u8 {
        let mut f = [0u8; 64];
        f[0..6].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]); // broadcast dst
        f[6..12].copy_from_slice(&[0x02, 0x00, 0x00, 0x00, 0x00, i]); // unique src
        hypervisor::vswitch::vswitch_forward(0, &f);
    }
    // All should have been learned (table has 16 slots)
    // Drain port 1 broadcasts
    while PORT_RX[1].take(&mut drain_buf).is_some() {}
    // Send to first learned MAC — should forward precisely
    let mut to_first = [0u8; 64];
    to_first[0..6].copy_from_slice(&[0x02, 0x00, 0x00, 0x00, 0x00, 0x00]); // dst = first
    to_first[6..12].copy_from_slice(&[0x02, 0x00, 0x00, 0x00, 0x00, 0xFF]); // src
    hypervisor::vswitch::vswitch_forward(1, &to_first);
    let len = PORT_RX[0].take(&mut buf);
    assert_ok(len.is_some(), "MAC table should hold 16 entries");
    uart_puts(b"[VSWITCH] Test 5 PASSED\n\n");

    uart_puts(b"========================================\n");
    uart_puts(b"  VSwitch Test PASSED (6 assertions)\n");
    uart_puts(b"========================================\n\n");
}

fn assert_ok(cond: bool, msg: &str) {
    if !cond {
        uart_puts(b"[VSWITCH] ASSERTION FAILED: ");
        uart_puts(msg.as_bytes());
        uart_puts(b"\n");
        panic!("test assertion failed");
    }
}
