//! The device interface and port mapping.
//!
//! Each CPT peripheral (display, keyboard, data disk, …) implements [`Device`]. The [`Machine`]
//! owns a set of devices and routes I/O ports to them; the executive loop ticks them with
//! elapsed cycles and collects any interrupt requests.
//!
//! [`Machine`]: crate::Machine

/// A memory-mapped-by-port peripheral.
pub trait Device {
    /// The I/O ports this device claims. The machine routes `io_in`/`io_out` for these ports
    /// here. Ports must not overlap between devices.
    fn ports(&self) -> &[u8];

    /// Handle an `IN` from one of this device's ports.
    fn io_in(&mut self, port: u8) -> u8;

    /// Handle an `OUT` to one of this device's ports.
    fn io_out(&mut self, port: u8, val: u8);

    /// Advance the device's internal time by `cycles` CPU cycles. Devices that generate
    /// periodic interrupts (a timer for the monitor, disk-completion, etc.) use this to decide
    /// when to raise an IRQ. Default: no-op.
    fn tick(&mut self, _cycles: u32) {}

    /// If the device is requesting an interrupt, return the RST vector (0..=7) to service and
    /// clear the request. Default: never interrupts.
    fn take_irq(&mut self) -> Option<u8> {
        None
    }

    /// A pending direct-memory-access transfer, if the device has one (e.g. a DMA controller
    /// moving a disk sector). The machine services it right after the device's `io_out` and on
    /// every tick, so the data is in memory before the CPU can observe completion. Default: none.
    fn take_dma(&mut self) -> Option<DmaRequest> {
        None
    }

    /// Delivers the bytes for a [`DmaRequest::ReadMemory`] the device asked for. Default: no-op.
    fn dma_read_done(&mut self, _data: &[u8]) {}

    /// Downcast support so hosts can reach a concrete device (e.g. the UI reading the display
    /// device's page buffer). Implementors return `self`.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

/// A DMA transfer between a device and memory, carried out by the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DmaRequest {
    /// Write `data` to memory starting at `addr` (device to memory, e.g. a disk read).
    WriteMemory { addr: u16, data: Vec<u8> },
    /// Read `len` bytes of memory starting at `addr` and hand them to the device with
    /// [`Device::dma_read_done`] (memory to device, e.g. a disk write).
    ReadMemory { addr: u16, len: usize },
}
