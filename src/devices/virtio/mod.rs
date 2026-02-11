//! Virtio device framework for the hypervisor.
//!
//! Implements the virtio-mmio transport layer and provides the `VirtioDevice`
//! trait for concrete device backends (e.g., virtio-blk).

pub mod queue;
pub mod mmio;
pub mod blk;

use queue::Virtqueue;

/// Trait for virtio device backends.
///
/// Implement this trait to create a new virtio device type. The transport
/// layer (`VirtioMmioTransport`) handles register access and virtqueue
/// management, delegating device-specific logic to this trait.
pub trait VirtioDevice {
    /// Virtio device ID (e.g., 2 for block device)
    fn device_id(&self) -> u32;

    /// Device feature bits (low 32 bits selected by feature_sel=0,
    /// high 32 bits by feature_sel=1)
    fn device_features(&self) -> u64;

    /// Read from device-specific config space.
    /// `offset` is relative to the config space base (MMIO offset 0x100).
    fn config_read(&self, offset: u64, size: u8) -> u64;

    /// Write to device-specific config space.
    fn config_write(&mut self, offset: u64, value: u64, size: u8);

    /// Handle a queue notification (doorbell write).
    /// Called when the guest writes to QueueNotify.
    /// The transport provides the queue so the device can process descriptors.
    fn queue_notify(&mut self, queue_idx: u16, queue: &mut Virtqueue);

    /// Number of virtqueues this device uses.
    fn num_queues(&self) -> u16;

    /// Maximum queue size (number of descriptors).
    fn max_queue_size(&self) -> u16 { 256 }
}
