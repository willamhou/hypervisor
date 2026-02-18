//! Virtio-MMIO transport layer (virtio spec v2, "modern" only).
//!
//! Implements the virtio-mmio register interface at a given base address.
//! Wraps a concrete `VirtioDevice` backend and handles feature negotiation,
//! queue setup, and interrupt signaling.

use crate::devices::MmioDevice;
use super::VirtioDevice;
use super::queue::Virtqueue;

/// Maximum number of virtqueues per device
const MAX_QUEUES: usize = 2;

// ── Virtio-MMIO register offsets ────────────────────────────────────
const MAGIC_VALUE: u64 = 0x000;
const VERSION: u64 = 0x004;
const DEVICE_ID: u64 = 0x008;
const VENDOR_ID: u64 = 0x00C;
const DEVICE_FEATURES: u64 = 0x010;
const DEVICE_FEATURES_SEL: u64 = 0x014;
const DRIVER_FEATURES: u64 = 0x020;
const DRIVER_FEATURES_SEL: u64 = 0x024;
const QUEUE_SEL: u64 = 0x030;
const QUEUE_NUM_MAX: u64 = 0x034;
const QUEUE_NUM: u64 = 0x038;
const QUEUE_READY: u64 = 0x044;
const QUEUE_NOTIFY: u64 = 0x050;
const INTERRUPT_STATUS: u64 = 0x060;
const INTERRUPT_ACK: u64 = 0x064;
const STATUS: u64 = 0x070;
const QUEUE_DESC_LOW: u64 = 0x080;
const QUEUE_DESC_HIGH: u64 = 0x084;
const QUEUE_DRIVER_LOW: u64 = 0x090;
const QUEUE_DRIVER_HIGH: u64 = 0x094;
const QUEUE_DEVICE_LOW: u64 = 0x0A0;
const QUEUE_DEVICE_HIGH: u64 = 0x0A4;
const CONFIG_GENERATION: u64 = 0x0FC;
const CONFIG_SPACE: u64 = 0x100;

// ── Magic and version ───────────────────────────────────────────────
const VIRTIO_MMIO_MAGIC: u32 = 0x74726976; // "virt"
const VIRTIO_MMIO_VERSION: u32 = 2;        // Modern (non-legacy)
const VIRTIO_VENDOR_ID: u32 = 0x554D4551;  // "QEMU"

// ── Interrupt status bits ───────────────────────────────────────────
const VIRTIO_INT_VRING: u32 = 1;

/// Virtio-MMIO transport wrapping a device backend.
pub struct VirtioMmioTransport<D: VirtioDevice> {
    /// MMIO base address
    base: u64,
    /// The device backend
    device: D,
    /// Virtqueues
    queues: [Virtqueue; MAX_QUEUES],
    /// Currently selected queue index
    queue_sel: u32,
    /// Device status register
    status: u32,
    /// Interrupt status (bits: 0=vring, 1=config change)
    interrupt_status: u32,
    /// Feature selection register (0=low 32 bits, 1=high 32 bits)
    device_features_sel: u32,
    /// Driver feature selection
    driver_features_sel: u32,
    /// Driver-acknowledged features
    driver_features: u64,
    /// Config space generation counter
    config_generation: u32,
    /// SPI INTID for this device (injected on completion)
    irq_intid: u32,
    /// Temporary storage for split 32-bit queue address writes
    queue_desc_high: u32,
    queue_driver_high: u32,
    queue_device_high: u32,
}

impl<D: VirtioDevice> VirtioMmioTransport<D> {
    pub fn new(base: u64, device: D, irq_intid: u32) -> Self {
        Self {
            base,
            device,
            queues: [Virtqueue::new(), Virtqueue::new()],
            queue_sel: 0,
            status: 0,
            interrupt_status: 0,
            device_features_sel: 0,
            driver_features_sel: 0,
            driver_features: 0,
            config_generation: 0,
            irq_intid,
            queue_desc_high: 0,
            queue_driver_high: 0,
            queue_device_high: 0,
        }
    }

    /// Get the currently selected queue (bounds-checked).
    fn current_queue(&self) -> Option<usize> {
        let idx = self.queue_sel as usize;
        if idx < MAX_QUEUES && idx < self.device.num_queues() as usize {
            Some(idx)
        } else {
            None
        }
    }

    /// Signal interrupt to guest by queuing SPI via global mechanism.
    fn signal_interrupt(&mut self) {
        self.interrupt_status |= VIRTIO_INT_VRING;
        crate::global::inject_spi(self.irq_intid);
    }

    /// Reset device to initial state.
    fn reset(&mut self) {
        self.status = 0;
        self.interrupt_status = 0;
        self.device_features_sel = 0;
        self.driver_features_sel = 0;
        self.driver_features = 0;
        self.queue_sel = 0;
        for q in &mut self.queues {
            q.reset();
        }
    }
}

impl<D: VirtioDevice> MmioDevice for VirtioMmioTransport<D> {
    fn read(&mut self, offset: u64, size: u8) -> Option<u64> {
        // Config space reads can be 1/2/4 bytes
        if offset >= CONFIG_SPACE {
            let config_off = offset - CONFIG_SPACE;
            return Some(self.device.config_read(config_off, size));
        }

        // All other registers are 32-bit
        if size != 4 {
            return Some(0);
        }

        let val = match offset {
            MAGIC_VALUE => VIRTIO_MMIO_MAGIC,
            VERSION => VIRTIO_MMIO_VERSION,
            DEVICE_ID => self.device.device_id(),
            VENDOR_ID => VIRTIO_VENDOR_ID,

            DEVICE_FEATURES => {
                let features = self.device.device_features();
                if self.device_features_sel == 0 {
                    features as u32
                } else {
                    (features >> 32) as u32
                }
            }

            QUEUE_NUM_MAX => {
                if self.current_queue().is_some() {
                    self.device.max_queue_size() as u32
                } else {
                    0
                }
            }

            QUEUE_READY => {
                if let Some(idx) = self.current_queue() {
                    self.queues[idx].ready as u32
                } else {
                    0
                }
            }

            INTERRUPT_STATUS => self.interrupt_status,
            STATUS => self.status,
            CONFIG_GENERATION => self.config_generation,

            _ => 0,
        };

        Some(val as u64)
    }

    fn write(&mut self, offset: u64, value: u64, size: u8) -> bool {
        // Config space writes
        if offset >= CONFIG_SPACE {
            let config_off = offset - CONFIG_SPACE;
            self.device.config_write(config_off, value, size);
            return true;
        }

        if size != 4 {
            return true;
        }

        let val = value as u32;

        match offset {
            DEVICE_FEATURES_SEL => {
                self.device_features_sel = val;
            }

            DRIVER_FEATURES => {
                if self.driver_features_sel == 0 {
                    self.driver_features = (self.driver_features & 0xFFFF_FFFF_0000_0000)
                        | (val as u64);
                } else {
                    self.driver_features = (self.driver_features & 0x0000_0000_FFFF_FFFF)
                        | ((val as u64) << 32);
                }
            }

            DRIVER_FEATURES_SEL => {
                self.driver_features_sel = val;
            }

            QUEUE_SEL => {
                self.queue_sel = val;
                // Clear stale high bits from previous queue setup
                self.queue_desc_high = 0;
                self.queue_driver_high = 0;
                self.queue_device_high = 0;
            }

            QUEUE_NUM => {
                if let Some(idx) = self.current_queue() {
                    self.queues[idx].num = val as u16;
                }
            }

            QUEUE_READY => {
                if let Some(idx) = self.current_queue() {
                    self.queues[idx].ready = val != 0;
                }
            }

            QUEUE_NOTIFY => {
                let queue_idx = val as u16;
                if (queue_idx as usize) < MAX_QUEUES
                    && (queue_idx as usize) < self.device.num_queues() as usize
                    && self.queues[queue_idx as usize].ready
                {
                    // Split borrow: take queue out, call device, put back
                    let q = &mut self.queues[queue_idx as usize];
                    self.device.queue_notify(queue_idx, q);
                    // Signal interrupt after processing
                    self.signal_interrupt();
                }
            }

            INTERRUPT_ACK => {
                self.interrupt_status &= !val;
            }

            STATUS => {
                if val == 0 {
                    self.reset();
                } else {
                    self.status = val;
                }
            }

            QUEUE_DESC_LOW => {
                if let Some(idx) = self.current_queue() {
                    self.queues[idx].set_desc_addr(val, self.queue_desc_high);
                }
            }
            QUEUE_DESC_HIGH => {
                self.queue_desc_high = val;
                if let Some(idx) = self.current_queue() {
                    // Re-set with updated high bits
                    let low = self.queues[idx].desc_addr_low();
                    self.queues[idx].set_desc_addr(low, val);
                }
            }

            QUEUE_DRIVER_LOW => {
                if let Some(idx) = self.current_queue() {
                    self.queues[idx].set_avail_addr(val, self.queue_driver_high);
                }
            }
            QUEUE_DRIVER_HIGH => {
                self.queue_driver_high = val;
                if let Some(idx) = self.current_queue() {
                    let low = self.queues[idx].avail_addr_low();
                    self.queues[idx].set_avail_addr(low, val);
                }
            }

            QUEUE_DEVICE_LOW => {
                if let Some(idx) = self.current_queue() {
                    self.queues[idx].set_used_addr(val, self.queue_device_high);
                }
            }
            QUEUE_DEVICE_HIGH => {
                self.queue_device_high = val;
                if let Some(idx) = self.current_queue() {
                    let low = self.queues[idx].used_addr_low();
                    self.queues[idx].set_used_addr(low, val);
                }
            }

            _ => {}
        }

        true
    }

    fn base_address(&self) -> u64 {
        self.base
    }

    fn size(&self) -> u64 {
        0x200 // 512 bytes per virtio-mmio spec
    }
}

/// Specialized methods for VirtioNet transport (RX injection).
impl VirtioMmioTransport<super::net::VirtioNet> {
    /// Inject a received frame into the guest's RX virtqueue.
    ///
    /// Writes a 12-byte virtio_net_hdr (zeroed, num_buffers=1) followed by
    /// the Ethernet frame data into the first available RX descriptor.
    /// Signals an interrupt (inject_spi) after writing.
    ///
    /// Returns false if no RX descriptor is available (guest hasn't
    /// replenished its RX queue).
    pub fn inject_rx(&mut self, frame: &[u8]) -> bool {
        let rx_queue = &mut self.queues[0];
        let chain = match rx_queue.get_avail_desc() {
            Some(c) => c,
            None => return false, // No available RX buffer
        };

        if chain.count == 0 {
            return false;
        }

        // virtio_net_hdr_v1: 12 bytes, all zeroed except num_buffers=1
        let hdr: [u8; 12] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0]; // num_buffers=1 (LE u16 at offset 10)

        let combined_len = hdr.len() + frame.len();

        // Check total writable capacity before writing
        let mut total_cap = 0usize;
        for i in 0..chain.count {
            let desc = &chain.descs[i];
            if desc.flags & super::queue::VIRTQ_DESC_F_WRITE != 0 {
                total_cap += desc.len as usize;
            }
        }
        if total_cap < combined_len {
            // Descriptor chain too small — return it with len=0 so guest can reuse
            rx_queue.put_used(chain.head, 0);
            return false;
        }

        // Write header + frame into descriptor buffer(s) using bulk copies
        let mut written = 0usize;

        for i in 0..chain.count {
            let desc = &chain.descs[i];
            if desc.flags & super::queue::VIRTQ_DESC_F_WRITE == 0 {
                continue;
            }
            let buf_addr = desc.addr as *mut u8;
            let buf_cap = desc.len as usize;
            let remaining = combined_len - written;
            let to_write = if remaining < buf_cap { remaining } else { buf_cap };

            unsafe {
                let dst = buf_addr;
                // Determine how much of header vs frame goes into this descriptor
                if written < 12 {
                    let hdr_remaining = 12 - written;
                    let hdr_bytes = if hdr_remaining < to_write { hdr_remaining } else { to_write };
                    core::ptr::copy_nonoverlapping(hdr.as_ptr().add(written), dst, hdr_bytes);
                    if to_write > hdr_bytes {
                        let frame_bytes = to_write - hdr_bytes;
                        core::ptr::copy_nonoverlapping(frame.as_ptr(), dst.add(hdr_bytes), frame_bytes);
                    }
                } else {
                    let frame_offset = written - 12;
                    core::ptr::copy_nonoverlapping(frame.as_ptr().add(frame_offset), dst, to_write);
                }
            }
            written += to_write;
        }

        rx_queue.put_used(chain.head, written as u32);
        self.signal_interrupt();
        true
    }
}
