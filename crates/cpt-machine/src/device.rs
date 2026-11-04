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

    /// Downcast support so hosts can reach a concrete device (e.g. the UI reading the display
    /// device's page buffer). Implementors return `self`.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}
