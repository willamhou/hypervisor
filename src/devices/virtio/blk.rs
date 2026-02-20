//! Virtio block device backend.
//!
//! Implements a simple virtio-blk device backed by an in-memory disk image.
//! The disk image is loaded into guest physical memory by QEMU's -device loader.
//! Since we use identity mapping, the hypervisor reads/writes the image directly.

use super::queue::Virtqueue;
use super::VirtioDevice;

// ── Virtio-blk request types ────────────────────────────────────────
const VIRTIO_BLK_T_IN: u32 = 0; // Read from disk
const VIRTIO_BLK_T_OUT: u32 = 1; // Write to disk
const VIRTIO_BLK_T_GET_ID: u32 = 8; // Get device ID string

// ── Virtio-blk status codes ────────────────────────────────────────
const VIRTIO_BLK_S_OK: u8 = 0;
const VIRTIO_BLK_S_IOERR: u8 = 1;
const VIRTIO_BLK_S_UNSUPP: u8 = 2;

// ── Virtio-blk feature bits ────────────────────────────────────────
const VIRTIO_BLK_F_SIZE_MAX: u64 = 1 << 1;
const VIRTIO_BLK_F_SEG_MAX: u64 = 1 << 2;
const VIRTIO_BLK_F_BLK_SIZE: u64 = 1 << 6;
const VIRTIO_F_VERSION_1: u64 = 1 << 32;

/// Virtio-blk request header (16 bytes, from guest memory).
#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioBlkReqHeader {
    req_type: u32,
    _reserved: u32,
    sector: u64,
}

/// Virtio-blk device backed by in-memory image.
pub struct VirtioBlk {
    /// Physical address of the disk image in memory
    disk_base: u64,
    /// Size of the disk image in bytes
    disk_size: u64,
    /// Capacity in 512-byte sectors
    capacity: u64,
}

impl VirtioBlk {
    /// Create a new virtio-blk device.
    ///
    /// `disk_base` is the physical address where the disk image is loaded.
    /// `disk_size` is the size of the disk image in bytes.
    pub fn new(disk_base: u64, disk_size: u64) -> Self {
        Self {
            disk_base,
            disk_size,
            capacity: disk_size / 512,
        }
    }

    /// Process a single virtio-blk request from a descriptor chain.
    fn process_request(
        &mut self,
        queue: &mut Virtqueue,
        head: u16,
        descs: &[super::queue::VirtqDesc],
        count: usize,
    ) {
        if count < 2 {
            // Need at least header + status
            return;
        }

        // Descriptor 0: request header (device-readable, 16 bytes)
        let hdr_addr = descs[0].addr;
        let header: VirtioBlkReqHeader =
            unsafe { core::ptr::read_volatile(hdr_addr as *const VirtioBlkReqHeader) };

        let mut status = VIRTIO_BLK_S_OK;
        let mut total_written = 0u32;

        match header.req_type {
            VIRTIO_BLK_T_IN => {
                // Read from disk: copy data from disk image to guest buffers
                let byte_offset = header.sector * 512;

                // Process data descriptors (all between header and status)
                let mut disk_off = byte_offset;
                for i in 1..count - 1 {
                    let desc = &descs[i];
                    let len = desc.len as u64;

                    if disk_off + len > self.disk_size {
                        status = VIRTIO_BLK_S_IOERR;
                        break;
                    }

                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            (self.disk_base + disk_off) as *const u8,
                            desc.addr as *mut u8,
                            len as usize,
                        );
                    }
                    disk_off += len;
                    total_written += desc.len;
                }
            }

            VIRTIO_BLK_T_OUT => {
                // Write to disk: copy data from guest buffers to disk image
                let byte_offset = header.sector * 512;

                let mut disk_off = byte_offset;
                for i in 1..count - 1 {
                    let desc = &descs[i];
                    let len = desc.len as u64;

                    if disk_off + len > self.disk_size {
                        status = VIRTIO_BLK_S_IOERR;
                        break;
                    }

                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            desc.addr as *const u8,
                            (self.disk_base + disk_off) as *mut u8,
                            len as usize,
                        );
                    }
                    disk_off += len;
                }
            }

            VIRTIO_BLK_T_GET_ID => {
                // Return a device ID string
                if count >= 3 {
                    let desc = &descs[1];
                    let id = b"hypervisor-vda\0\0\0\0\0\0";
                    let copy_len = core::cmp::min(desc.len as usize, 20);
                    unsafe {
                        core::ptr::copy_nonoverlapping(id.as_ptr(), desc.addr as *mut u8, copy_len);
                    }
                    total_written = copy_len as u32;
                }
            }

            _ => {
                status = VIRTIO_BLK_S_UNSUPP;
            }
        }

        // Last descriptor: status byte (device-writable, 1 byte)
        let status_desc = &descs[count - 1];
        unsafe {
            core::ptr::write_volatile(status_desc.addr as *mut u8, status);
        }
        total_written += 1; // status byte

        queue.put_used(head, total_written);
    }
}

impl VirtioDevice for VirtioBlk {
    fn device_id(&self) -> u32 {
        2
    } // VIRTIO_ID_BLOCK

    fn device_features(&self) -> u64 {
        VIRTIO_F_VERSION_1 | VIRTIO_BLK_F_BLK_SIZE | VIRTIO_BLK_F_SIZE_MAX | VIRTIO_BLK_F_SEG_MAX
    }

    fn config_read(&self, offset: u64, size: u8) -> u64 {
        // Virtio-blk config space layout:
        //   0x00: capacity (u64, in 512-byte sectors)
        //   0x08: size_max (u32)
        //   0x0C: seg_max (u32)
        //   0x14: blk_size (u32) — at offset 0x14 in the spec
        match (offset, size) {
            // capacity: 64-bit at offset 0
            (0, 4) => self.capacity as u32 as u64,
            (4, 4) => (self.capacity >> 32) as u32 as u64,
            (0, 8) => self.capacity,
            // size_max: 32-bit at offset 8
            (8, 4) => 0x0020_0000, // 2MB max segment
            // seg_max: 32-bit at offset 12
            (12, 4) => 128,
            // blk_size: 32-bit at offset 20
            (20, 4) => 512,
            _ => 0,
        }
    }

    fn config_write(&mut self, _offset: u64, _value: u64, _size: u8) {
        // Config space is read-only for blk
    }

    fn queue_notify(&mut self, _queue_idx: u16, queue: &mut Virtqueue) {
        // Process all available descriptor chains
        while let Some(chain) = queue.get_avail_desc() {
            self.process_request(queue, chain.head, &chain.descs, chain.count);
        }
    }

    fn num_queues(&self) -> u16 {
        1
    } // Single request queue

    fn max_queue_size(&self) -> u16 {
        256
    }
}
