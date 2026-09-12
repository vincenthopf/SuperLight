use crate::{native::macos_ffi::*, transport::Candidate};
use std::{cell::UnsafeCell, ffi::{CString, c_void}, io, ptr, thread::{self, ThreadId}, time::{Duration, Instant}};
use superlight_core::{devices, reports::PacketQueue};

struct Manager(Owned);

impl Manager {
    fn new(candidate: Option<&Candidate>) -> io::Result<Self> {
        let manager = Owned::new(unsafe { IOHIDManagerCreate(ptr::null(), 0) })?;
        let matching = dictionary()?;
        let mut properties = vec![("VendorID", 0x046d)];
        if let Some(candidate) = candidate {
            properties.push(("ProductID", i32::from(candidate.product_id)));
            if candidate.usage_page != 0 { properties.push(("PrimaryUsagePage", i32::from(candidate.usage_page))); }
            if candidate.usage != 0 { properties.push(("PrimaryUsage", i32::from(candidate.usage))); }
        }
        for (key, value) in properties {
            let key = string(key)?;
            let value = number(value)?;
            unsafe { CFDictionarySetValue(matching.0, key.0, value.0); }
        }
        unsafe { IOHIDManagerSetDeviceMatching(manager.0, matching.0); }
        let result = unsafe { IOHIDManagerOpen(manager.0, 0) };
        if result != 0 { return Err(os_error("Open Logitech IOHID manager", result)); }
        Ok(Self(manager))
    }

    fn devices(&self) -> io::Result<Vec<Owned>> {
        let raw = unsafe { IOHIDManagerCopyDevices(self.0.0) };
        if raw.is_null() { return Ok(Vec::new()); }
        let set = Owned::new(raw)?;
        let count = unsafe { CFSetGetCount(set.0) };
        if !(0..=256).contains(&count) { return Err(io::Error::other("The Logitech device inventory exceeds its safety limit")); }
        let mut pointers = vec![ptr::null(); count as usize];
        unsafe { CFSetGetValues(set.0, pointers.as_mut_ptr()); }
        pointers.into_iter().map(|device| unsafe { Owned::retained(device) }).collect()
    }
}

impl Drop for Manager {
    fn drop(&mut self) { unsafe { IOHIDManagerClose(self.0.0, 0); } }
}

fn property(device: Cf, name: &str) -> io::Result<Cf> {
    let name = string(name)?;
    Ok(unsafe { IOHIDDeviceGetProperty(device, name.0) })
}

fn identity(device: Cf) -> io::Result<u64> {
    let service = unsafe { IOHIDDeviceGetService(device) };
    if service == 0 { return Err(io::Error::other("The Logitech device has no registry identity")); }
    let mut identity = 0;
    let result = unsafe { IORegistryEntryGetRegistryEntryID(service, &mut identity) };
    if result != 0 || identity == 0 { return Err(os_error("Read Logitech registry identity", result)); }
    Ok(identity)
}

pub(crate) fn enumerate() -> io::Result<Vec<Candidate>> {
    let manager = Manager::new(None)?;
    let mut result = Vec::new();
    for device in manager.devices()? {
        let vendor = unsafe { integer(property(device.0, "VendorID")?) };
        let product = unsafe { integer(property(device.0, "ProductID")?) };
        let page = unsafe { integer(property(device.0, "PrimaryUsagePage")?) };
        let usage = unsafe { integer(property(device.0, "PrimaryUsage")?) };
        let name = unsafe { text(property(device.0, "Product")?) };
        if !(0..=65535).contains(&product) || !(0..=65535).contains(&page) || !(0..=65535).contains(&usage)
            || !devices::candidate_allowed(vendor as u16, product as u16, &name, page as u16) { continue; }
        let Ok(identifier) = identity(device.0) else { continue; };
        let bus = unsafe { text(property(device.0, "Transport")?) };
        result.push(Candidate {
            path: CString::new(format!("iokit:{identifier}")).map_err(io::Error::other)?,
            product_id: product as u16, name: name.chars().take(255).collect(),
            bluetooth: bus.starts_with("Bluetooth") || (0xb000..=0xbfff).contains(&product),
            usage_page: page as u16, usage: usage as u16, interface: -2,
        });
    }
    Ok(result)
}

#[derive(Default)]
struct CallbackState {
    queue: PacketQueue,
    removed: bool,
    fault: Option<i32>,
    overflow: bool,
}

impl CallbackState {
    fn input(&mut self, result: i32, report_type: u32, report_id: u32, bytes: &[u8]) {
        if report_type != 0 || !matches!(report_id, 0 | 0x10 | 0x11) { return; }
        if result != 0 { self.fault = Some(result); return; }
        if self.queue.push(bytes).is_err() { self.overflow = true; }
    }

    fn read(&mut self, buffer: &mut [u8; 64]) -> io::Result<Option<usize>> {
        if self.removed { return Err(io::Error::new(io::ErrorKind::NotConnected, "The Logitech IOHID device disconnected")); }
        if self.overflow { return Err(io::Error::other("The bounded Logitech report queue overflowed. Reconnecting to release captured input.")); }
        if let Some(fault) = self.fault { return Err(os_error("Read Logitech IOHID report", fault)); }
        Ok(self.queue.pop_into(buffer))
    }
}

unsafe extern "C" fn input_callback(context: *mut c_void, result: i32, _: Cf, report_type: u32, report_id: u32, bytes: *mut u8, length: isize) {
    if context.is_null() { return; }
    let state = context.cast::<CallbackState>();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !(1..=64).contains(&length) || bytes.is_null() {
            if report_type == 0 && matches!(report_id, 0x10 | 0x11) { unsafe { (*state).overflow = true; } }
            return;
        }
        unsafe { (*state).input(result, report_type, report_id, std::slice::from_raw_parts(bytes, length as usize)); }
    }));
    if result.is_err() { unsafe { (*state).overflow = true; } }
}

unsafe extern "C" fn removal_callback(context: *mut c_void, _: i32, _: Cf) {
    if !context.is_null() { unsafe { (*context.cast::<CallbackState>()).removed = true; } }
}

pub(crate) struct Device {
    device: Owned,
    _manager: Manager,
    state: Box<UnsafeCell<CallbackState>>,
    _buffer: Box<[u8; 512]>,
    loop_ref: Cf,
    owner: ThreadId,
}

impl Device {
    pub(crate) fn open(candidate: &Candidate) -> io::Result<Self> {
        let expected = candidate.path.to_str().ok().and_then(|path| path.strip_prefix("iokit:")).and_then(|identifier| identifier.parse::<u64>().ok()).ok_or_else(|| io::Error::other("Invalid Logitech IOHID identity"))?;
        let manager = Manager::new(Some(candidate))?;
        let device = manager.devices()?.into_iter().find(|device| identity(device.0).ok() == Some(expected)).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "The selected Logitech IOHID interface is no longer present"))?;
        let state = Box::new(UnsafeCell::new(CallbackState::default()));
        let mut buffer = Box::new([0; 512]);
        let loop_ref = unsafe { CFRunLoopGetCurrent() };
        let result = unsafe { IOHIDDeviceOpen(device.0, 0) };
        if result != 0 { return Err(os_error("Open shared Logitech IOHID device", result)); }
        unsafe {
            IOHIDDeviceRegisterInputReportCallback(device.0, buffer.as_mut_ptr(), buffer.len() as isize, input_callback, state.get().cast());
            IOHIDDeviceRegisterRemovalCallback(device.0, removal_callback, state.get().cast());
            IOHIDDeviceScheduleWithRunLoop(device.0, loop_ref, kCFRunLoopDefaultMode);
        }
        Ok(Self { device, _manager: manager, state, _buffer: buffer, loop_ref, owner: thread::current().id() })
    }

    fn thread_check(&self) -> io::Result<()> {
        if thread::current().id() != self.owner { return Err(io::Error::other("The Logitech IOHID handle must stay on its owning thread")); }
        Ok(())
    }

    pub(crate) fn write(&mut self, report: &[u8; 20]) -> io::Result<usize> {
        self.thread_check()?;
        let result = unsafe { IOHIDDeviceSetReport(self.device.0, 1, isize::from(report[0]), report.as_ptr(), report.len() as isize) };
        if result != 0 { return Err(os_error("Write Logitech IOHID report", result)); }
        Ok(report.len())
    }

    pub(crate) fn read(&mut self, buffer: &mut [u8; 64], timeout: Duration) -> io::Result<usize> {
        self.thread_check()?;
        let start = Instant::now();
        loop {
            if let Some(length) = unsafe { (&mut *self.state.get()).read(buffer)? } { return Ok(length); }
            let Some(remaining) = timeout.checked_sub(start.elapsed()).filter(|remaining| !remaining.is_zero()) else { return Ok(0); };
            let result = unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, remaining.min(Duration::from_millis(50)).as_secs_f64(), 1) };
            if matches!(result, 1 | 2) { return Err(io::Error::new(io::ErrorKind::NotConnected, "The Logitech IOHID run loop stopped")); }
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            IOHIDDeviceUnscheduleFromRunLoop(self.device.0, self.loop_ref, kCFRunLoopDefaultMode);
            IOHIDDeviceRegisterInputReportCallback(self.device.0, ptr::null_mut(), 0, input_callback, ptr::null_mut());
            IOHIDDeviceRegisterRemovalCallback(self.device.0, removal_callback, ptr::null_mut());
            IOHIDDeviceClose(self.device.0, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_reports_preserve_both_report_id_variants() {
        let mut state = CallbackState::default();
        let with_id = [0x11, 0xff, 7, 0x0a, 1, 2];
        let without_id = [0xff, 7, 0x0a, 1, 2];
        state.input(0, 0, 0x11, &with_id);
        state.input(0, 0, 0x11, &without_id);
        let mut buffer = [0; 64];
        assert_eq!(state.read(&mut buffer).unwrap(), Some(with_id.len()));
        assert_eq!(&buffer[..with_id.len()], &with_id);
        assert_eq!(state.read(&mut buffer).unwrap(), Some(without_id.len()));
        assert_eq!(&buffer[..without_id.len()], &without_id);
    }

    #[test]
    fn unrelated_input_report_types_are_ignored() {
        let mut state = CallbackState::default();
        state.input(0, 1, 0x11, &[1, 2, 3]);
        state.input(0, 0, 1, &[1, 2, 3]);
        assert_eq!(state.read(&mut [0; 64]).unwrap(), None);
    }

    #[test]
    fn queue_overflow_and_removal_fail_closed_before_old_input_is_delivered() {
        let mut state = CallbackState::default();
        for _ in 0..=superlight_core::reports::PACKET_CAPACITY { state.input(0, 0, 0x11, &[0xff, 7, 0, 0, 0xc3]); }
        assert!(state.read(&mut [0; 64]).is_err());
        let mut state = CallbackState::default();
        state.input(0, 0, 0x11, &[0xff, 7, 0, 0, 0xc3]);
        state.removed = true;
        assert!(state.read(&mut [0; 64]).is_err());
    }
}
