use crate::{hid_access::Access, shared::Shared};
use hidapi::{BusType, HidApi, HidDevice};
use std::{
    ffi::CString,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use superlight_core::{
    devices, hidpp,
    session::{Error, Transport},
};

pub type AccessHandle = Arc<Mutex<Access>>;
pub const MAX_CANDIDATES: usize = 32;

#[derive(Clone, Debug)]
pub struct Candidate {
    pub path: CString,
    pub product_id: u16,
    pub name: String,
    pub bluetooth: bool,
    pub usage_page: u16,
    pub usage: u16,
    pub interface: i32,
}

impl Candidate {
    pub fn priority(&self) -> (u8, u8, u8, i32) {
        (
            u8::from(!self.bluetooth),
            u8::from(self.usage_page < 0xff00),
            u8::from(self.usage != 2),
            self.interface,
        )
    }

    pub fn indices(&self) -> &'static [u8] {
        if self.bluetooth {
            &[0xff]
        } else {
            &hidpp::DEVICE_INDICES
        }
    }

    pub fn backend(&self) -> &'static str {
        if cfg!(target_os = "macos") && self.path.as_bytes().starts_with(b"iokit:") {
            "IOKit"
        } else {
            "hidapi"
        }
    }
}

pub fn enumerate(api: &mut HidApi) -> Result<Vec<Candidate>, Error> {
    let refresh_error = api.refresh_devices().err();
    let mut candidates = Vec::new();
    if refresh_error.is_none() {
        for info in api.device_list() {
            let name = info.product_string().unwrap_or_default();
            if !devices::candidate_allowed(
                info.vendor_id(),
                info.product_id(),
                name,
                info.usage_page(),
            ) {
                continue;
            }
            let candidate = Candidate {
                path: info.path().to_owned(),
                product_id: info.product_id(),
                name: name.chars().take(255).collect(),
                bluetooth: matches!(info.bus_type(), BusType::Bluetooth)
                    || (0xb000..=0xbfff).contains(&info.product_id()),
                usage_page: info.usage_page(),
                usage: info.usage(),
                interface: info.interface_number(),
            };
            if candidates
                .iter()
                .any(|existing: &Candidate| existing.path == candidate.path)
            {
                continue;
            }
            candidates.push(candidate);
        }
    }
    #[cfg(target_os = "macos")]
    if let Ok(native) = crate::macos_hid::enumerate() {
        candidates.extend(native);
    }
    if candidates.is_empty()
        && let Some(error) = refresh_error
    {
        return Err(Error::Transport(error.to_string()));
    }
    candidates.sort_by(|a, b| {
        a.priority()
            .cmp(&b.priority())
            .then_with(|| a.path.cmp(&b.path))
    });
    candidates.dedup_by(|a, b| a.path == b.path);
    candidates.truncate(MAX_CANDIDATES);
    Ok(candidates)
}

enum Device {
    Hidapi(HidDevice),
    #[cfg(target_os = "macos")]
    Iokit(crate::macos_hid::Device),
}

pub struct HidTransport {
    device: Device,
    shared: Weak<Shared>,
    access: AccessHandle,
}

impl HidTransport {
    pub fn open(
        api: &HidApi,
        candidate: &Candidate,
        shared: &Arc<Shared>,
    ) -> Result<(Self, AccessHandle), Error> {
        #[cfg(target_os = "macos")]
        if candidate.backend() == "IOKit" {
            let device = crate::macos_hid::Device::open(candidate)
                .map_err(|error| Error::Transport(error.to_string()))?;
            return Ok(Self::with_device(Device::Iokit(device), shared));
        }
        let device = api
            .open_path(&candidate.path)
            .map_err(|error| Error::Transport(error.to_string()))?;
        device
            .set_blocking_mode(true)
            .map_err(|error| Error::Transport(error.to_string()))?;
        Ok(Self::with_device(Device::Hidapi(device), shared))
    }

    fn with_device(device: Device, shared: &Arc<Shared>) -> (Self, AccessHandle) {
        let access = Arc::new(Mutex::new(Access::default()));
        (
            Self {
                device,
                shared: Arc::downgrade(shared),
                access: Arc::clone(&access),
            },
            access,
        )
    }
}

impl Transport for HidTransport {
    fn write_report(&mut self, report: &[u8; 20]) -> Result<(), Error> {
        self.access
            .lock()
            .map_err(|_| Error::Transport("HID access policy is unavailable".into()))?
            .validate(report)
            .map_err(Error::Transport)?;
        let written = match &mut self.device {
            Device::Hidapi(device) => device
                .write(report)
                .map_err(|error| Error::Transport(error.to_string()))?,
            #[cfg(target_os = "macos")]
            Device::Iokit(device) => device
                .write(report)
                .map_err(|error| Error::Transport(error.to_string()))?,
        };
        if written != report.len() {
            return Err(Error::Transport(format!(
                "Incomplete HID write: {written} of {} bytes",
                report.len()
            )));
        }
        Ok(())
    }

    fn read_wait(&mut self, buffer: &mut [u8; 64], timeout: Duration) -> Result<usize, Error> {
        if self.shared.upgrade().is_none_or(|shared| shared.stopping()) {
            return Err(Error::Transport("The service is stopping".into()));
        }
        let milliseconds = timeout.as_millis().clamp(1, 250) as i32;
        let count = match &mut self.device {
            Device::Hidapi(device) => device
                .read_timeout(buffer, milliseconds)
                .map_err(|error| Error::Transport(error.to_string()))?,
            #[cfg(target_os = "macos")]
            Device::Iokit(device) => device
                .read(buffer, Duration::from_millis(milliseconds as u64))
                .map_err(|error| Error::Transport(error.to_string()))?,
        };
        if count > buffer.len() {
            return Err(Error::Transport("Invalid HID report length".into()));
        }
        self.access
            .lock()
            .map_err(|_| Error::Transport("HID access policy is unavailable".into()))?
            .observe(&buffer[..count]);
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(bluetooth: bool, page: u16, usage: u16) -> Candidate {
        Candidate {
            path: CString::new("test").unwrap(),
            product_id: if bluetooth { 0xb034 } else { 0xc548 },
            name: String::new(),
            bluetooth,
            usage_page: page,
            usage,
            interface: 1,
        }
    }

    #[test]
    fn bluetooth_precedes_receivers_and_vendor_collections_precede_mouse_collections() {
        assert!(candidate(true, 0, 0).priority() < candidate(false, 0xff00, 2).priority());
        assert!(candidate(false, 0xff00, 2).priority() < candidate(false, 1, 2).priority());
        assert!(candidate(false, 0xff00, 2).priority() < candidate(false, 0xff00, 1).priority());
    }

    #[test]
    fn bluetooth_only_probes_direct_index_and_receivers_probe_all_six_slots() {
        assert_eq!(candidate(true, 0, 0).indices(), &[0xff]);
        assert_eq!(
            candidate(false, 0xff00, 2).indices(),
            &[0xff, 1, 2, 3, 4, 5, 6]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_iohid_interfaces_are_identified_without_changing_receiver_addressing() {
        let mut native = candidate(true, 0xff00, 2);
        native.path = CString::new("iokit:42").unwrap();
        native.interface = -2;
        assert_eq!(native.backend(), "IOKit");
        assert_eq!(native.indices(), &[0xff]);
        assert!(native.priority() < candidate(true, 0xff00, 2).priority());
    }
}
