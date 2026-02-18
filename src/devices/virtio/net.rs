//! Virtio network device backend.
//!
//! Implements a virtio-net device (device ID 1) for inter-VM networking.
//! TX: strips virtio_net_hdr, forwards Ethernet frame via VSwitch.
//! RX: inject_rx() writes virtio_net_hdr + frame into guest RX queue.

use super::VirtioDevice;
use super::queue::Virtqueue;

// ── Feature bits ────────────────────────────────────────────────────
const VIRTIO_NET_F_MAC: u64 = 1 << 5;
const VIRTIO_NET_F_STATUS: u64 = 1 << 16;
const VIRTIO_F_VERSION_1: u64 = 1 << 32;

// Status bits
const VIRTIO_NET_S_LINK_UP: u16 = 1;

/// Size of virtio_net_hdr_v1 (with num_buffers field).
/// Linux always uses this size for VERSION_1 devices.
const VIRTIO_NET_HDR_SIZE: usize = 12;

/// Virtio-net device backend.
pub struct VirtioNet {
    mac: [u8; 6],
    port_id: usize,
    status: u16,
}

impl VirtioNet {
    /// Create a new VirtioNet device for the given VM.
    pub fn new(vm_id: usize) -> Self {
        Self {
            mac: Self::mac_for_vm(vm_id),
            port_id: vm_id,
            status: VIRTIO_NET_S_LINK_UP,
        }
    }

    /// Generate a deterministic MAC address for a VM.
    /// VM 0 -> 52:54:00:00:00:01, VM 1 -> 52:54:00:00:00:02
    pub fn mac_for_vm(vm_id: usize) -> [u8; 6] {
        [0x52, 0x54, 0x00, 0x00, 0x00, (vm_id + 1) as u8]
    }

    /// Process TX queue: strip virtio_net_hdr, forward frames via VSwitch.
    fn process_tx(&mut self, queue: &mut Virtqueue) {
        while let Some(chain) = queue.get_avail_desc() {
            // Descriptor chain: [virtio_net_hdr] [frame data...]
            // Could be 1 descriptor (hdr + frame) or 2+ (hdr, then frame)
            let mut total_len = 0usize;
            let mut frame_buf = [0u8; crate::vswitch::MAX_FRAME_SIZE];
            let mut frame_len = 0usize;

            for i in 0..chain.count {
                let desc = &chain.descs[i];
                let buf_addr = desc.addr as *const u8;
                let buf_len = desc.len as usize;

                if total_len < VIRTIO_NET_HDR_SIZE {
                    // Still in the header region — skip header bytes
                    let skip = core::cmp::min(VIRTIO_NET_HDR_SIZE - total_len, buf_len);
                    let data_start = skip;
                    let data_len = buf_len - skip;
                    if data_len > 0 && frame_len + data_len <= frame_buf.len() {
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                buf_addr.add(data_start),
                                frame_buf.as_mut_ptr().add(frame_len),
                                data_len,
                            );
                        }
                        frame_len += data_len;
                    }
                } else {
                    // Pure frame data
                    if frame_len + buf_len <= frame_buf.len() {
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                buf_addr,
                                frame_buf.as_mut_ptr().add(frame_len),
                                buf_len,
                            );
                        }
                        frame_len += buf_len;
                    }
                }
                total_len += buf_len;
            }

            // Forward the Ethernet frame through the VSwitch
            if frame_len >= 14 {
                crate::vswitch::vswitch_forward(self.port_id, &frame_buf[..frame_len]);
            }

            queue.put_used(chain.head, 0);
        }
    }
}

impl VirtioDevice for VirtioNet {
    fn device_id(&self) -> u32 { 1 } // VIRTIO_ID_NET

    fn device_features(&self) -> u64 {
        VIRTIO_F_VERSION_1 | VIRTIO_NET_F_MAC | VIRTIO_NET_F_STATUS
    }

    fn config_read(&self, offset: u64, size: u8) -> u64 {
        // Config space layout:
        //   0x00-0x05: mac[6]     (6 bytes)
        //   0x06-0x07: status     (u16)
        match (offset, size) {
            // Single byte reads of MAC address
            (o @ 0..=5, 1) => self.mac[o as usize] as u64,
            // 2-byte read of status
            (6, 2) => self.status as u64,
            // 4-byte read spanning MAC bytes
            (0, 4) => {
                (self.mac[0] as u64)
                    | ((self.mac[1] as u64) << 8)
                    | ((self.mac[2] as u64) << 16)
                    | ((self.mac[3] as u64) << 24)
            }
            (4, 4) => {
                (self.mac[4] as u64)
                    | ((self.mac[5] as u64) << 8)
                    | ((self.status as u64) << 16)
            }
            _ => 0,
        }
    }

    fn config_write(&mut self, _offset: u64, _value: u64, _size: u8) {
        // Config space is read-only for net
    }

    fn queue_notify(&mut self, queue_idx: u16, queue: &mut Virtqueue) {
        match queue_idx {
            0 => {} // RX queue — guest replenishing buffers, no action needed
            1 => self.process_tx(queue),
            _ => {}
        }
    }

    fn num_queues(&self) -> u16 { 2 } // RX=0, TX=1

    fn max_queue_size(&self) -> u16 { 256 }
}
