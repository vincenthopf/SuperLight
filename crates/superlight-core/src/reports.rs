use crate::{
    hidpp,
    session::{Divert, Report},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceEvent {
    Button { source: u8, down: bool },
    Motion { x: i16, y: i16 },
}

pub struct Notifications {
    device: u8,
    feature: u8,
    diverts: [Option<Divert>; 3],
    held: [bool; 3],
}

impl Notifications {
    pub fn new(device: u8, feature: u8) -> Self {
        Self {
            device,
            feature,
            diverts: [None; 3],
            held: [false; 3],
        }
    }

    pub fn configure(&mut self, diverts: [Option<Divert>; 3]) {
        for (index, divert) in diverts.iter().enumerate() {
            if *divert != self.diverts[index] {
                self.held[index] = false;
            }
        }
        self.diverts = diverts;
    }

    pub fn clear(&mut self) {
        self.held.fill(false);
    }

    pub fn process(&mut self, report: Report, mut emit: impl FnMut(DeviceEvent)) {
        if report.device != self.device
            || report.feature != self.feature
            || report.software != hidpp::NOTIFICATION_SOFTWARE
        {
            return;
        }
        if report.function == hidpp::DIVERTED_BUTTONS_EVENT && report.len >= 2 {
            for (index, source) in [1, 6, 7].into_iter().enumerate() {
                let down = self.diverts[index]
                    .is_some_and(|divert| hidpp::contains_cid(report.params(), divert.cid));
                if down != self.held[index] {
                    self.held[index] = down;
                    emit(DeviceEvent::Button { source, down });
                }
            }
        } else if report.function == hidpp::RAW_XY_EVENT
            && self.held[0]
            && self.diverts[0].is_some_and(|divert| divert.raw_xy)
            && let Some((x, y)) = hidpp::signed_xy(report.params())
        {
            emit(DeviceEvent::Motion { x, y });
        }
    }
}

#[derive(Clone, Copy)]
struct Packet {
    bytes: [u8; 64],
    len: u8,
}

const EMPTY_PACKET: Packet = Packet {
    bytes: [0; 64],
    len: 0,
};
pub const PACKET_CAPACITY: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueFault {
    Full,
    InvalidLength,
}

pub struct PacketQueue {
    packets: [Packet; PACKET_CAPACITY],
    head: usize,
    len: usize,
}

impl Default for PacketQueue {
    fn default() -> Self {
        Self {
            packets: [EMPTY_PACKET; PACKET_CAPACITY],
            head: 0,
            len: 0,
        }
    }
}

impl PacketQueue {
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), QueueFault> {
        if bytes.is_empty() || bytes.len() > 64 {
            return Err(QueueFault::InvalidLength);
        }
        if self.len == PACKET_CAPACITY {
            return Err(QueueFault::Full);
        }
        let index = (self.head + self.len) % PACKET_CAPACITY;
        self.packets[index].bytes[..bytes.len()].copy_from_slice(bytes);
        self.packets[index].len = bytes.len() as u8;
        self.len += 1;
        Ok(())
    }

    pub fn pop_into(&mut self, buffer: &mut [u8; 64]) -> Option<usize> {
        if self.len == 0 {
            return None;
        }
        let packet = self.packets[self.head];
        let len = usize::from(packet.len);
        buffer[..len].copy_from_slice(&packet.bytes[..len]);
        self.head = (self.head + 1) % PACKET_CAPACITY;
        self.len -= 1;
        Some(len)
    }
}
