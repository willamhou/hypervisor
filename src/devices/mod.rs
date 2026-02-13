//! Device Emulation Framework
//!
//! Routes MMIO accesses to emulated devices via enum dispatch.
//! Devices are registered dynamically into an array of up to `MAX_DEVICES` slots.

pub mod pl011;
pub mod gic;
pub mod virtio;

/// Trait for MMIO-accessible devices
///
/// - `read()`/`write()` receive offsets relative to `base_address()`
/// - `size` parameter indicates access width (1, 2, 4, or 8 bytes)
/// - Return `None`/`false` for invalid offsets
pub trait MmioDevice {
    fn read(&mut self, offset: u64, size: u8) -> Option<u64>;
    fn write(&mut self, offset: u64, value: u64, size: u8) -> bool;
    fn base_address(&self) -> u64;
    fn size(&self) -> u64;

    fn contains(&self, addr: u64) -> bool {
        let base = self.base_address();
        addr >= base && addr < base + self.size()
    }

    /// Return a pending SPI INTID if the device wants to assert an interrupt.
    fn pending_irq(&self) -> Option<u32> { None }

    /// Acknowledge/clear the device-side interrupt.
    fn ack_irq(&mut self) { }
}

// ── Enum dispatch ──────────────────────────────────────────────────

/// Device variant enum — one variant per supported device type.
/// Adding a new device requires adding a variant here.
pub enum Device {
    Uart(pl011::VirtualUart),
    Gicd(gic::VirtualGicd),
    Gicr(gic::VirtualGicr),
    VirtioBlk(virtio::mmio::VirtioMmioTransport<virtio::blk::VirtioBlk>),
}

impl MmioDevice for Device {
    fn read(&mut self, offset: u64, size: u8) -> Option<u64> {
        match self {
            Device::Uart(d) => d.read(offset, size),
            Device::Gicd(d) => d.read(offset, size),
            Device::Gicr(d) => d.read(offset, size),
            Device::VirtioBlk(d) => d.read(offset, size),
        }
    }

    fn write(&mut self, offset: u64, value: u64, size: u8) -> bool {
        match self {
            Device::Uart(d) => d.write(offset, value, size),
            Device::Gicd(d) => d.write(offset, value, size),
            Device::Gicr(d) => d.write(offset, value, size),
            Device::VirtioBlk(d) => d.write(offset, value, size),
        }
    }

    fn base_address(&self) -> u64 {
        match self {
            Device::Uart(d) => d.base_address(),
            Device::Gicd(d) => d.base_address(),
            Device::Gicr(d) => d.base_address(),
            Device::VirtioBlk(d) => d.base_address(),
        }
    }

    fn size(&self) -> u64 {
        match self {
            Device::Uart(d) => d.size(),
            Device::Gicd(d) => d.size(),
            Device::Gicr(d) => d.size(),
            Device::VirtioBlk(d) => d.size(),
        }
    }

    fn pending_irq(&self) -> Option<u32> {
        match self {
            Device::Uart(d) => d.pending_irq(),
            Device::Gicd(d) => d.pending_irq(),
            Device::Gicr(d) => d.pending_irq(),
            Device::VirtioBlk(d) => d.pending_irq(),
        }
    }

    fn ack_irq(&mut self) {
        match self {
            Device::Uart(d) => d.ack_irq(),
            Device::Gicd(d) => d.ack_irq(),
            Device::Gicr(d) => d.ack_irq(),
            Device::VirtioBlk(d) => d.ack_irq(),
        }
    }
}

// ── Device Manager ─────────────────────────────────────────────────

const MAX_DEVICES: usize = 8;

/// Virtio-blk MMIO base address (first QEMU virt virtio-mmio slot)
const VIRTIO_BLK_BASE: u64 = 0x0a00_0000;
/// Virtio-blk SPI: SPI 16 = INTID 48
const VIRTIO_BLK_INTID: u32 = 48;

/// MMIO Device Manager — routes accesses to registered devices by address.
pub struct DeviceManager {
    devices: [Option<Device>; MAX_DEVICES],
    count: usize,
}

impl DeviceManager {
    pub const fn new() -> Self {
        Self {
            devices: [const { None }; MAX_DEVICES],
            count: 0,
        }
    }

    /// Remove all registered devices.
    pub fn reset(&mut self) {
        for slot in self.devices.iter_mut() {
            *slot = None;
        }
        self.count = 0;
    }

    /// Register a device. Returns slot index on success.
    pub fn register_device(&mut self, dev: Device) -> Option<usize> {
        if self.count >= MAX_DEVICES {
            return None;
        }
        let idx = self.count;
        self.devices[idx] = Some(dev);
        self.count += 1;
        Some(idx)
    }

    /// Attach a virtio-blk device backed by an in-memory disk image.
    pub fn attach_virtio_blk(&mut self, disk_base: u64, disk_size: u64) {
        let blk = virtio::blk::VirtioBlk::new(disk_base, disk_size);
        let transport = virtio::mmio::VirtioMmioTransport::new(
            VIRTIO_BLK_BASE, blk, VIRTIO_BLK_INTID,
        );
        self.register_device(Device::VirtioBlk(transport));
    }

    /// Handle MMIO access by scanning registered devices.
    pub fn handle_mmio(&mut self, addr: u64, value: u64, size: u8, is_write: bool) -> Option<u64> {
        for slot in self.devices.iter_mut() {
            if let Some(dev) = slot {
                if dev.contains(addr) {
                    let offset = addr - dev.base_address();
                    return if is_write {
                        dev.write(offset, value, size);
                        None
                    } else {
                        dev.read(offset, size)
                    };
                }
            }
        }
        // Unknown device — return 0 for reads, ignore writes
        if is_write { None } else { Some(0) }
    }

    /// Look up SPI routing via GICD_IROUTER.
    pub fn route_spi(&self, intid: u32) -> usize {
        for slot in &self.devices {
            if let Some(Device::Gicd(gicd)) = slot {
                return gicd.route_spi(intid);
            }
        }
        0
    }

    /// Get a mutable reference to the UART device (for RX injection).
    pub fn uart_mut(&mut self) -> Option<&mut pl011::VirtualUart> {
        for slot in self.devices.iter_mut() {
            if let Some(Device::Uart(uart)) = slot {
                return Some(uart);
            }
        }
        None
    }
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}
