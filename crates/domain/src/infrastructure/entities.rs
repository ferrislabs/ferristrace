use super::value_objects::{ResourceKind, SILENT_SECS};

const NANOS_PER_SECOND: u64 = 1_000_000_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Resource {
    pub kind: ResourceKind,
    pub id: Box<str>,
    pub name: Box<str>,
    pub cluster: Box<str>,
    pub environment: Box<str>,
    pub provider: Box<str>,
    pub region: Box<str>,
    pub zone: Box<str>,
    pub host_type: Box<str>,
    pub first_seen_unix_nanos: u64,
    pub last_seen_unix_nanos: u64,
    pub silent: bool,
    pub cpu_percent: Option<f64>,
    pub memory_bytes: Option<u64>,
}

impl Resource {
    pub fn settled(mut self, now_unix_nanos: u64) -> Self {
        let horizon = now_unix_nanos.saturating_sub(SILENT_SECS * NANOS_PER_SECOND);
        self.silent = self.last_seen_unix_nanos < horizon;
        if self.silent {
            self.cpu_percent = None;
            self.memory_bytes = None;
        }
        self
    }
}
