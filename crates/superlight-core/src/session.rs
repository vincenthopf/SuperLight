use crate::hidpp::{self, Control, ProtocolError, ResponseMatch, SmartShift};
use std::{
    fmt,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Transport(String),
    Protocol(ProtocolError),
    Timeout,
    Unsupported,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => f.write_str(error),
            Self::Protocol(error) => error.fmt(f),
            Self::Timeout => f.write_str("The Logitech device did not reply before the deadline"),
            Self::Unsupported => f.write_str("The Logitech device does not support this feature"),
        }
    }
}

impl std::error::Error for Error {}
impl From<ProtocolError> for Error {
    fn from(error: ProtocolError) -> Self {
        Self::Protocol(error)
    }
}

pub trait Transport {
    fn write_report(&mut self, report: &[u8; 20]) -> Result<(), Error>;
    fn read_wait(&mut self, buffer: &mut [u8; 64], timeout: Duration) -> Result<usize, Error>;
}

impl<T: Transport + ?Sized> Transport for Box<T> {
    fn write_report(&mut self, report: &[u8; 20]) -> Result<(), Error> {
        (**self).write_report(report)
    }
    fn read_wait(&mut self, buffer: &mut [u8; 64], timeout: Duration) -> Result<usize, Error> {
        (**self).read_wait(buffer, timeout)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub device: u8,
    pub feature: u8,
    pub function: u8,
    pub software: u8,
    pub parameters: [u8; 16],
    pub len: u8,
}

impl Report {
    pub fn from_message(message: hidpp::Message<'_>) -> Self {
        let len = message.params.len().min(16);
        let mut parameters = [0; 16];
        parameters[..len].copy_from_slice(&message.params[..len]);
        Self {
            device: message.device,
            feature: message.feature,
            function: message.function,
            software: message.software,
            parameters,
            len: len as u8,
        }
    }

    pub fn params(&self) -> &[u8] {
        &self.parameters[..usize::from(self.len)]
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Features {
    pub reprog: Option<u8>,
    pub dpi: Option<u8>,
    pub smart_shift: Option<u8>,
    pub enhanced_smart_shift: bool,
    pub battery: Option<u8>,
    pub unified_battery: bool,
    pub name: Option<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Divert {
    pub cid: u16,
    pub raw_xy: bool,
}

pub struct Session<T: Transport> {
    transport: Option<T>,
    pub device_index: u8,
    pub features: Features,
    pub timeout: Duration,
    timeouts: u8,
    diverts: [Option<Divert>; 3],
}

impl<T: Transport> Session<T> {
    pub fn new(transport: T, device_index: u8) -> Self {
        Self {
            transport: Some(transport),
            device_index,
            features: Features::default(),
            timeout: Duration::from_secs(2),
            timeouts: 0,
            diverts: [None; 3],
        }
    }

    pub fn retarget(&mut self, device_index: u8) -> Result<(), Error> {
        if self.diverts.iter().any(Option::is_some) {
            return Err(ProtocolError::Malformed.into());
        }
        self.device_index = device_index;
        self.features = Features::default();
        self.timeouts = 0;
        Ok(())
    }

    pub fn needs_reconnect(&self) -> bool {
        self.timeouts >= 3
    }
    pub fn diverts(&self) -> &[Option<Divert>; 3] {
        &self.diverts
    }

    fn transport(&mut self) -> Result<&mut T, Error> {
        self.transport
            .as_mut()
            .ok_or_else(|| Error::Transport("HID transport is closed".into()))
    }

    pub fn request(
        &mut self,
        feature: u8,
        function: u8,
        parameters: &[u8],
        notify: &mut impl FnMut(Report),
    ) -> Result<Report, Error> {
        self.request_with_timeout(feature, function, parameters, self.timeout, notify)
    }

    pub fn request_with_timeout(
        &mut self,
        feature: u8,
        function: u8,
        parameters: &[u8],
        timeout: Duration,
        notify: &mut impl FnMut(Report),
    ) -> Result<Report, Error> {
        let report = hidpp::encode(self.device_index, feature, function, parameters)?;
        self.transport()?.write_report(&report)?;
        let started = Instant::now();
        let mut buffer = [0; 64];
        while let Some(remaining) = timeout.checked_sub(started.elapsed()) {
            if remaining.is_zero() {
                break;
            }
            let len = self
                .transport()?
                .read_wait(&mut buffer, remaining.min(Duration::from_millis(250)))?;
            if len > buffer.len() {
                return Err(ProtocolError::Malformed.into());
            }
            let Some(message) = hidpp::parse(&buffer[..len]) else {
                continue;
            };
            match hidpp::match_response(message, self.device_index, feature, function) {
                ResponseMatch::Reply => {
                    self.timeouts = 0;
                    return Ok(Report::from_message(message));
                }
                ResponseMatch::Error(code) => {
                    self.timeouts = 0;
                    return Err(ProtocolError::Device(code).into());
                }
                ResponseMatch::Unrelated => {
                    if message.device == self.device_index && message.software == 0 {
                        notify(Report::from_message(message));
                    }
                }
            }
        }
        self.timeouts = self.timeouts.saturating_add(1);
        Err(Error::Timeout)
    }

    pub fn read_notification(
        &mut self,
        timeout: Duration,
        notify: &mut impl FnMut(Report),
    ) -> Result<bool, Error> {
        let mut buffer = [0; 64];
        let len = self.transport()?.read_wait(&mut buffer, timeout)?;
        if len > buffer.len() {
            return Err(ProtocolError::Malformed.into());
        }
        if let Some(message) = hidpp::parse(&buffer[..len])
            && message.device == self.device_index
            && message.software == 0
        {
            notify(Report::from_message(message));
        }
        Ok(len != 0)
    }

    pub fn feature(
        &mut self,
        id: u16,
        notify: &mut impl FnMut(Report),
    ) -> Result<Option<u8>, Error> {
        let [high, low] = id.to_be_bytes();
        match self.request(0, 0, &[high, low, 0], notify) {
            Ok(report) => Ok(report.params().first().copied().filter(|index| *index != 0)),
            Err(Error::Protocol(ProtocolError::Device(6 | 7 | 9))) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn discover_controls(
        &mut self,
        notify: &mut impl FnMut(Report),
    ) -> Result<Vec<Control>, Error> {
        let feature = self.features.reprog.ok_or(Error::Unsupported)?;
        let response = self.request(feature, 0, &[], notify)?;
        let count =
            usize::from(response.params().first().copied().unwrap_or(0)).min(hidpp::MAX_CONTROLS);
        let mut controls = Vec::with_capacity(count);
        let mut failures = 0;
        for index in 0..count {
            let response = match self.request_with_timeout(
                feature,
                1,
                &[index as u8],
                Duration::from_millis(500),
                notify,
            ) {
                Ok(response) => response,
                Err(Error::Transport(error)) => return Err(Error::Transport(error)),
                Err(_) => {
                    failures += 1;
                    if failures >= 3 {
                        break;
                    }
                    continue;
                }
            };
            failures = 0;
            let Some(mut control) = Control::decode(index as u8, response.params()) else {
                continue;
            };
            if let Ok(reporting) = self.request(feature, 2, &control.cid.to_be_bytes(), notify) {
                control.apply_reporting(reporting.params());
            }
            controls.push(control);
        }
        Ok(controls)
    }

    pub fn read_name(&mut self, notify: &mut impl FnMut(Report)) -> Result<String, Error> {
        let feature = self.features.name.ok_or(Error::Unsupported)?;
        let response = self.request(feature, 0, &[], notify)?;
        let len = usize::from(response.params().first().copied().unwrap_or(0));
        let mut bytes = Vec::with_capacity(len);
        while bytes.len() < len {
            let response = self.request(feature, 1, &[bytes.len() as u8], notify)?;
            if response.len == 0 {
                break;
            }
            let take = (len - bytes.len()).min(response.params().len());
            bytes.extend_from_slice(&response.params()[..take]);
        }
        Ok(String::from_utf8_lossy(&bytes)
            .trim_matches('\0')
            .trim()
            .to_owned())
    }

    pub fn discover_optional_features(
        &mut self,
        notify: &mut impl FnMut(Report),
    ) -> Result<(), Error> {
        self.features.dpi = self.feature(hidpp::DPI, notify)?;
        self.features.smart_shift = self.feature(hidpp::SMART_SHIFT_ENHANCED, notify)?;
        self.features.enhanced_smart_shift = self.features.smart_shift.is_some();
        if self.features.smart_shift.is_none() {
            self.features.smart_shift = self.feature(hidpp::SMART_SHIFT, notify)?;
        }
        self.features.battery = self.feature(hidpp::UNIFIED_BATTERY, notify)?;
        self.features.unified_battery = self.features.battery.is_some();
        if self.features.battery.is_none() {
            self.features.battery = self.feature(hidpp::BATTERY_STATUS, notify)?;
        }
        Ok(())
    }

    fn set_reporting(
        &mut self,
        cid: u16,
        flags: u8,
        notify: &mut impl FnMut(Report),
    ) -> Result<(), Error> {
        let feature = self.features.reprog.ok_or(Error::Unsupported)?;
        let [high, low] = cid.to_be_bytes();
        self.request(feature, 3, &[high, low, flags, 0, 0], notify)?;
        Ok(())
    }

    pub fn divert_gesture(
        &mut self,
        candidates: &[u16],
        notify: &mut impl FnMut(Report),
    ) -> Result<Option<Divert>, Error> {
        if self.diverts[0].is_some() {
            return Ok(self.diverts[0]);
        }
        for &cid in candidates.iter().take(hidpp::MAX_CONTROLS) {
            for (flags, raw_xy) in [(0x33, true), (0x03, false)] {
                match self.set_reporting(cid, flags, notify) {
                    Ok(()) => {
                        self.diverts[0] = Some(Divert { cid, raw_xy });
                        return Ok(self.diverts[0]);
                    }
                    Err(Error::Transport(error)) => return Err(Error::Transport(error)),
                    Err(_) if self.needs_reconnect() => return Err(Error::Timeout),
                    Err(_) => {}
                }
            }
        }
        Ok(None)
    }

    pub fn divert_extra(&mut self, cid: u16, notify: &mut impl FnMut(Report)) -> Result<(), Error> {
        let slot = match cid {
            hidpp::MODE_SHIFT_CID => 1,
            hidpp::DPI_SWITCH_CID => 2,
            _ => return Err(Error::Unsupported),
        };
        if self.diverts[slot].is_some() {
            return Ok(());
        }
        self.set_reporting(cid, 0x03, notify)?;
        self.diverts[slot] = Some(Divert { cid, raw_xy: false });
        Ok(())
    }

    pub fn undivert_slot(&mut self, slot: usize) -> Result<(), Error> {
        let Some(divert) = self.diverts.get(slot).copied().flatten() else {
            return Ok(());
        };
        let feature = self.features.reprog.ok_or(Error::Unsupported)?;
        let [high, low] = divert.cid.to_be_bytes();
        let flags = if divert.raw_xy { 0x22 } else { 0x02 };
        let report = hidpp::encode(self.device_index, feature, 3, &[high, low, flags, 0, 0])?;
        self.transport()?.write_report(&report)?;
        self.diverts[slot] = None;
        Ok(())
    }

    pub fn shutdown(&mut self) {
        for slot in [1, 2, 0] {
            let _ = self.undivert_slot(slot);
        }
    }

    pub fn set_dpi(&mut self, dpi: u16, notify: &mut impl FnMut(Report)) -> Result<(), Error> {
        let feature = self.features.dpi.ok_or(Error::Unsupported)?;
        let [high, low] = dpi.to_be_bytes();
        self.request(feature, 3, &[0, high, low], notify)?;
        Ok(())
    }

    pub fn read_dpi(&mut self, notify: &mut impl FnMut(Report)) -> Result<u16, Error> {
        let feature = self.features.dpi.ok_or(Error::Unsupported)?;
        let response = self.request(feature, 2, &[0], notify)?;
        let params = response.params();
        if params.len() < 3 {
            return Err(ProtocolError::Malformed.into());
        }
        Ok(u16::from_be_bytes([params[1], params[2]]))
    }

    pub fn set_smart_shift(
        &mut self,
        state: SmartShift,
        notify: &mut impl FnMut(Report),
    ) -> Result<(), Error> {
        let feature = self.features.smart_shift.ok_or(Error::Unsupported)?;
        let function = if self.features.enhanced_smart_shift {
            2
        } else {
            1
        };
        self.request(feature, function, &state.wire_parameters(), notify)?;
        Ok(())
    }

    pub fn read_smart_shift(
        &mut self,
        notify: &mut impl FnMut(Report),
    ) -> Result<SmartShift, Error> {
        let feature = self.features.smart_shift.ok_or(Error::Unsupported)?;
        let function = u8::from(self.features.enhanced_smart_shift);
        let response = self.request(feature, function, &[], notify)?;
        let params = response.params();
        if params.len() < 2 {
            return Err(ProtocolError::Malformed.into());
        }
        Ok(SmartShift::decode(params[0], params[1]))
    }

    pub fn read_battery(&mut self, notify: &mut impl FnMut(Report)) -> Result<u8, Error> {
        let feature = self.features.battery.ok_or(Error::Unsupported)?;
        let function = u8::from(self.features.unified_battery);
        let response = self.request(feature, function, &[], notify)?;
        response
            .params()
            .first()
            .copied()
            .filter(|percent| *percent <= 100)
            .ok_or(ProtocolError::Malformed.into())
    }
}

impl<T: Transport> Drop for Session<T> {
    fn drop(&mut self) {
        self.shutdown();
    }
}
