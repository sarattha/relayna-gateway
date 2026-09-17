//! Bounded transport state. No frame payloads or prompts are retained here.
use gateway_core::{GatewayError, GatewayResult};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

pub const INSTANCE_CONNECTION_LIMIT: usize = 4096;

#[derive(Debug, Default)]
pub struct ConnectionLimits(Mutex<HashMap<(String, Uuid), usize>>);

impl ConnectionLimits {
    pub fn acquire(
        self: &Arc<Self>,
        service: &str,
        key: Uuid,
        route_max: usize,
        key_max: usize,
    ) -> GatewayResult<ConnectionLease> {
        let mut counts = self
            .0
            .lock()
            .map_err(|_| GatewayError::ControlStateUnavailable)?;
        let total: usize = counts.values().sum();
        let route: usize = counts
            .iter()
            .filter(|((name, _), _)| name == service)
            .map(|(_, count)| count)
            .sum();
        let entry = (service.to_owned(), key);
        if total >= INSTANCE_CONNECTION_LIMIT
            || route >= route_max
            || counts.get(&entry).copied().unwrap_or(0) >= key_max
        {
            return Err(GatewayError::RateLimitExceeded {
                retry_after_seconds: Some(1),
            });
        }
        *counts.entry(entry.clone()).or_default() += 1;
        Ok(ConnectionLease {
            limits: self.clone(),
            entry,
        })
    }
}

#[derive(Debug)]
pub struct ConnectionLease {
    limits: Arc<ConnectionLimits>,
    entry: (String, Uuid),
}
impl Drop for ConnectionLease {
    fn drop(&mut self) {
        if let Ok(mut counts) = self.limits.0.lock() {
            if let Some(count) = counts.get_mut(&self.entry) {
                *count -= 1;
                if *count == 0 {
                    counts.remove(&self.entry);
                }
            }
        }
    }
}

/// Parse only frame headers, skipping payloads. Bounds frames AND fragmented messages.
#[derive(Debug)]
pub struct FrameLimit {
    pub frames: u64,
    pub close_seen: bool,
    header: Vec<u8>,
    remaining: u64,
    message_bytes: u64,
    fragmented: bool,
    max: u64,
    masked: bool,
}
impl FrameLimit {
    pub fn new(max: usize, masked: bool) -> Self {
        Self {
            frames: 0,
            close_seen: false,
            header: Vec::with_capacity(14),
            remaining: 0,
            message_bytes: 0,
            fragmented: false,
            max: max as u64,
            masked,
        }
    }
    pub fn feed(&mut self, mut bytes: &[u8]) -> GatewayResult<()> {
        while !bytes.is_empty() {
            if self.remaining != 0 {
                let skip = self.remaining.min(bytes.len() as u64) as usize;
                self.remaining -= skip as u64;
                bytes = &bytes[skip..];
                continue;
            }
            self.header.push(bytes[0]);
            bytes = &bytes[1..];
            if self.header.len() < 2 {
                continue;
            }
            let marker = self.header[1] & 127;
            let extended = match marker {
                126 => 2,
                127 => 8,
                _ => 0,
            };
            let header_len = 2 + extended + if self.masked { 4 } else { 0 };
            if self.header.len() < header_len {
                continue;
            }
            let mut size = if extended == 0 { marker as u64 } else { 0 };
            for byte in &self.header[2..2 + extended] {
                size = (size << 8) | *byte as u64;
            }
            let opcode = self.header[0] & 15;
            let fin = self.header[0] & 128 != 0;
            let invalid = self.header[0] & 112 != 0
                || (self.header[1] & 128 != 0) != self.masked
                || size > self.max
                || size > i64::MAX as u64
                || (marker == 126 && size < 126)
                || (marker == 127 && size <= 65535);
            if invalid {
                return Err(GatewayError::RequestBodyTooLarge);
            }
            match opcode {
                0 if self.fragmented => {
                    self.message_bytes = self.message_bytes.saturating_add(size);
                }
                1 | 2 if !self.fragmented => {
                    self.message_bytes = size;
                }
                8..=10 if fin && size <= 125 => {}
                _ => return Err(GatewayError::InvalidServicePayload),
            }
            if opcode < 8 {
                if self.message_bytes > self.max {
                    return Err(GatewayError::RequestBodyTooLarge);
                }
                self.fragmented = !fin;
                if fin {
                    self.message_bytes = 0;
                }
            }
            self.frames = self.frames.saturating_add(1);
            self.close_seen |= opcode == 8;
            self.remaining = size;
            self.header.clear();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counts_split_frame_headers_and_close_without_retaining_payloads() {
        let mut frames = FrameLimit::new(1024, false);
        for byte in [0x81, 3, b'a', b'b', b'c', 0x89, 0, 0x88, 0] {
            frames.feed(&[byte]).unwrap();
        }
        assert_eq!(frames.frames, 3);
        assert!(frames.close_seen);
        assert!(frames.header.is_empty());
    }
    #[test]
    fn connection_limits_release_on_all_drop_paths() {
        let limits = Arc::new(ConnectionLimits::default());
        let key = Uuid::new_v4();
        let lease = limits.acquire("tara", key, 2, 1).unwrap();
        assert!(limits.acquire("tara", key, 2, 1).is_err());
        let other = limits.acquire("tara", Uuid::new_v4(), 2, 1).unwrap();
        assert!(limits.acquire("tara", Uuid::new_v4(), 2, 1).is_err());
        let another = limits.acquire("docgen", key, 2, 1).unwrap();
        assert!(format!("{lease:?}").contains("ConnectionLease"));
        drop((lease, other, another));
        assert!(limits.0.lock().unwrap().is_empty());
        let one = limits.acquire("tara", key, 2, 2).unwrap();
        let two = limits.acquire("tara", key, 2, 2).unwrap();
        drop(one);
        assert_eq!(limits.0.lock().unwrap().values().sum::<usize>(), 1);
        drop(two);
        limits
            .0
            .lock()
            .unwrap()
            .insert(("full".into(), key), INSTANCE_CONNECTION_LIMIT);
        assert!(limits.acquire("other", key, 10, 10).is_err());
    }
    fn frame(op: u8, payload: usize, masked: bool) -> Vec<u8> {
        let mut bytes = vec![op];
        let mask = if masked { 128 } else { 0 };
        if payload < 126 {
            bytes.push(mask | payload as u8);
        } else if payload <= 65535 {
            bytes.push(mask | 126);
            bytes.extend_from_slice(&(payload as u16).to_be_bytes());
        } else {
            bytes.push(mask | 127);
            bytes.extend_from_slice(&(payload as u64).to_be_bytes());
        }
        if masked {
            bytes.extend_from_slice(&[0; 4]);
        }
        bytes.resize(bytes.len() + payload, 0);
        bytes
    }
    #[test]
    fn parses_split_headers_without_retaining_payloads() {
        for masked in [true, false] {
            for size in [0, 1, 125, 126, 65535, 65536] {
                let mut limit = FrameLimit::new(70000, masked);
                let data = frame(0x81, size, masked);
                for chunk in data.chunks(3) {
                    limit.feed(chunk).unwrap();
                }
                assert_eq!(limit.remaining, 0);
                assert!(limit.header.is_empty());
                limit.feed(&frame(0x82, 10, masked)).unwrap();
                limit.feed(&frame(0x89, 1, masked)).unwrap();
                assert!(format!("{limit:?}").contains("FrameLimit"));
            }
        }
    }
    #[test]
    fn rejects_oversized_and_malformed_messages() {
        let mut limit = FrameLimit::new(125, false);
        limit.feed(&frame(1, 100, false)).unwrap();
        limit.feed(&frame(0x89, 0, false)).unwrap();
        assert!(limit.feed(&frame(0x80, 26, false)).is_err());
        let mut limit = FrameLimit::new(125, false);
        limit.feed(&frame(2, 50, false)).unwrap();
        limit.feed(&frame(0, 25, false)).unwrap();
        limit.feed(&frame(0x80, 50, false)).unwrap();
        limit.feed(&frame(0x81, 125, false)).unwrap();
        for data in [
            frame(0x81, 126, false),
            frame(0x80, 1, false),
            frame(0x83, 1, false),
            frame(0x89, 126, false),
            frame(9, 0, false),
            frame(0xc1, 0, false),
            frame(0x81, 1, true),
            vec![0x81, 126, 0, 1],
            vec![0x81, 127, 0, 0, 0, 0, 0, 0, 0, 1],
            vec![0x81, 127, 255, 255, 255, 255, 255, 255, 255, 255],
        ] {
            assert!(FrameLimit::new(125, false).feed(&data).is_err(), "{data:?}");
        }
        let mut limit = FrameLimit::new(125, false);
        limit.feed(&frame(1, 1, false)).unwrap();
        assert!(limit.feed(&frame(0x81, 1, false)).is_err());
    }
}
