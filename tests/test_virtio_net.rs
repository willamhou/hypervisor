//! VirtioNet device backend tests

use hypervisor::devices::virtio::net::VirtioNet;
use hypervisor::devices::virtio::VirtioDevice;
use hypervisor::uart_puts;

pub fn run_virtio_net_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  VirtioNet Device Test\n");
    uart_puts(b"========================================\n\n");

    let net = VirtioNet::new(0);

    // Test 1: device_id
    uart_puts(b"[VNET] Test 1: device_id...\n");
    assert_eq_vnet(net.device_id(), 1, "device_id should be 1 (VIRTIO_ID_NET)");
    uart_puts(b"[VNET] Test 1 PASSED\n\n");

    // Test 2: device_features (VERSION_1 | MAC | STATUS, no CSUM)
    uart_puts(b"[VNET] Test 2: device_features...\n");
    let features = net.device_features();
    let version_1: u64 = 1 << 32;
    let mac: u64 = 1 << 5;
    let status: u64 = 1 << 16;
    let csum: u64 = 1 << 0;
    assert_eq_vnet(features & version_1, version_1, "VERSION_1 should be set");
    assert_eq_vnet(features & mac, mac, "MAC should be set");
    assert_eq_vnet(features & status, status, "STATUS should be set");
    assert_eq_vnet(features & csum, 0, "CSUM should NOT be set");
    uart_puts(b"[VNET] Test 2 PASSED\n\n");

    // Test 3: num_queues
    uart_puts(b"[VNET] Test 3: num_queues...\n");
    assert_eq_vnet(net.num_queues(), 2, "should have 2 queues (RX + TX)");
    uart_puts(b"[VNET] Test 3 PASSED\n\n");

    // Test 4: config_read MAC bytes
    uart_puts(b"[VNET] Test 4: config_read MAC...\n");
    // MAC for VM 0: 52:54:00:00:00:01
    let byte0 = net.config_read(0, 1);
    assert_eq_vnet(byte0, 0x52, "MAC[0] should be 0x52");
    let byte5 = net.config_read(5, 1);
    assert_eq_vnet(byte5, 0x01, "MAC[5] should be 0x01");
    uart_puts(b"[VNET] Test 4 PASSED\n\n");

    // Test 5: config_read status (LINK_UP = 1)
    uart_puts(b"[VNET] Test 5: config_read status...\n");
    let status_val = net.config_read(6, 2);
    assert_eq_vnet(status_val, 1, "status should be LINK_UP (1)");
    uart_puts(b"[VNET] Test 5 PASSED\n\n");

    // Test 6: mac_for_vm
    uart_puts(b"[VNET] Test 6: mac_for_vm...\n");
    let mac0 = VirtioNet::mac_for_vm(0);
    assert_eq_vnet(mac0, [0x52, 0x54, 0x00, 0x00, 0x00, 0x01], "VM 0 MAC");
    let mac1 = VirtioNet::mac_for_vm(1);
    assert_eq_vnet(mac1, [0x52, 0x54, 0x00, 0x00, 0x00, 0x02], "VM 1 MAC");
    uart_puts(b"[VNET] Test 6 PASSED\n\n");

    uart_puts(b"========================================\n");
    uart_puts(b"  VirtioNet Device Test PASSED (8 assertions)\n");
    uart_puts(b"========================================\n\n");
}

fn assert_eq_vnet<T: PartialEq + core::fmt::Debug>(a: T, b: T, msg: &str) {
    if a != b {
        uart_puts(b"[VNET] ASSERTION FAILED: ");
        uart_puts(msg.as_bytes());
        uart_puts(b"\n");
        panic!("test assertion failed");
    }
}
