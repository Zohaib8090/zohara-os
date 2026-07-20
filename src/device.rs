// src/device.rs

//! Device manager — central registry for kernel devices.
//!
//! Each driver registers itself with a name, type, and capabilities.
//! Future: PCI, USB, SATA, NVMe, Network, GPU.

/// Device types.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq)]
pub enum DeviceType {
    Serial,
    Timer,
    Keyboard,
    Storage,
    Network,
    Gpu,
    Unknown,
}

impl DeviceType {
    pub fn as_str(self) -> &'static str {
        match self {
            DeviceType::Serial  => "serial",
            DeviceType::Timer   => "timer",
            DeviceType::Keyboard => "keyboard",
            DeviceType::Storage => "storage",
            DeviceType::Network => "network",
            DeviceType::Gpu     => "gpu",
            DeviceType::Unknown => "unknown",
        }
    }
}

/// A registered device.
#[derive(Copy, Clone)]
pub struct Device {
    pub name: &'static str,
    pub dev_type: DeviceType,
    pub id: usize,
}

/// Maximum number of registered devices.
const MAX_DEVICES: usize = 32;

static mut DEVICES: [Option<Device>; MAX_DEVICES] = [None; MAX_DEVICES];
static mut DEVICE_COUNT: usize = 0;

/// Register a device. Returns its ID.
pub fn register_device(name: &'static str, dev_type: DeviceType) -> usize {
    unsafe {
        let id = DEVICE_COUNT;
        if id >= MAX_DEVICES {
            crate::warn!("devmgr", "device table full, cannot register '{}'", name);
            return usize::MAX;
        }
        DEVICES[id] = Some(Device { name, dev_type, id });
        DEVICE_COUNT += 1;
        crate::info!("devmgr", "registered device '{}' (type={}, id={})", name, dev_type.as_str(), id);
        id
    }
}

/// Look up a device by name.
pub fn find_device(name: &str) -> Option<usize> {
    unsafe {
        for i in 0..DEVICE_COUNT {
            if let Some(ref dev) = DEVICES[i] {
                if dev.name == name {
                    return Some(dev.id);
                }
            }
        }
        None
    }
}

/// Print all registered devices.
pub fn list_devices() {
    unsafe {
        crate::println!("=== Registered Devices ===");
        for i in 0..DEVICE_COUNT {
            if let Some(ref dev) = DEVICES[i] {
                crate::println!("  [{}] {} ({})", dev.id, dev.name, dev.dev_type.as_str());
            }
        }
    }
}
