//! Validated monitoring configuration and deterministic target state machine.
use crate::{
    destination::Target,
    error::{Error, Result},
};
use serde::{Deserialize, Serialize};

pub const CONFIRM_MS: i64 = 30_000;
pub const HOUR_MS: i64 = 3_600_000;
pub const DAY_MS: i64 = 86_400_000;
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Http,
    Tcp,
    Ping,
}
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    #[default]
    Auto,
    Ipv4,
    Ipv6,
}
fn interval() -> u32 {
    60
}
fn deadline() -> u32 {
    5
}
fn threshold() -> u8 {
    3
}
fn yes() -> bool {
    true
}
fn statuses() -> Vec<[u16; 2]> {
    vec![[200, 299]]
}
/// Deliberately no Debug: target URLs can contain sensitive query parameters.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MonitorConfig {
    pub name: String,
    pub kind: Kind,
    pub target: String,
    pub port: Option<u16>,
    #[serde(default = "interval")]
    pub interval_seconds: u32,
    #[serde(default = "deadline")]
    pub deadline_seconds: u32,
    #[serde(default = "threshold")]
    pub failure_threshold: u8,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "yes")]
    pub alerts: bool,
    #[serde(default)]
    pub address_family: Family,
    #[serde(default = "statuses")]
    pub accepted_statuses: Vec<[u16; 2]>,
}
impl MonitorConfig {
    pub fn validate(&self) -> Result<Target> {
        if !(1..=100).contains(&self.name.chars().count())
            || self.name.trim().is_empty()
            || self.name.chars().any(char::is_control)
            || !(30..=3600).contains(&self.interval_seconds)
            || !(1..=30).contains(&self.deadline_seconds)
            || self.deadline_seconds >= self.interval_seconds
            || !(1..=5).contains(&self.failure_threshold)
            || self.accepted_statuses.is_empty()
            || self.accepted_statuses.len() > 32
            || self
                .accepted_statuses
                .iter()
                .any(|[lo, hi]| *lo < 100 || *hi > 599 || lo > hi)
        {
            return Err(Error::InvalidInput);
        }
        match self.kind {
            Kind::Http if self.port.is_none() => Target::http(&self.target),
            Kind::Tcp => Target::tcp(&self.target, self.port.ok_or(Error::InvalidInput)?),
            Kind::Ping if self.port.is_none() => Target::ping(&self.target),
            _ => Err(Error::InvalidInput),
        }
    }
    pub fn load(&self) -> f64 {
        if !self.enabled {
            return 0.0;
        }
        let period = if self.failure_threshold > 1 {
            self.interval_seconds.min(30)
        } else {
            self.interval_seconds
        };
        f64::from(self.deadline_seconds) / f64::from(period)
    }
    pub fn accepts(&self, code: u16) -> bool {
        self.accepted_statuses
            .iter()
            .any(|[lo, hi]| (*lo..=*hi).contains(&code))
    }
    pub fn interval_ms(&self) -> i64 {
        i64::from(self.interval_seconds) * 1000
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Success,
    Failure,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Up,
    PendingFailure,
    Down,
    Unknown,
    Paused,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cadence {
    Normal,
    Confirmation,
}
/// Only fixed safe categories cross the persistence/API boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeError {
    Dns,
    Timeout,
    Connection,
    Tls,
    Headers,
    HttpStatus,
    Echo,
    Permission,
    Unsupported,
    Policy,
    DnsLimit,
    LocalCapacity,
    Scheduler,
    ClockJump,
}
impl ProbeError {
    pub fn outcome(self) -> Outcome {
        match self {
            Self::Permission
            | Self::Unsupported
            | Self::Policy
            | Self::DnsLimit
            | Self::LocalCapacity
            | Self::Scheduler
            | Self::ClockJump => Outcome::Unknown,
            _ => Outcome::Failure,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ProbeResult {
    pub outcome: Outcome,
    pub elapsed_ms: u32,
    pub status_code: Option<u16>,
    pub error: Option<ProbeError>,
    pub destination_family: Option<Family>,
}
impl ProbeResult {
    pub fn fault(error: ProbeError) -> Self {
        Self {
            outcome: error.outcome(),
            elapsed_ms: 0,
            status_code: None,
            error: Some(error),
            destination_family: None,
        }
    }
    pub fn success() -> Self {
        Self {
            outcome: Outcome::Success,
            elapsed_ms: 0,
            status_code: None,
            error: None,
            destination_family: None,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Monitor {
    pub id: i64,
    pub revision: i64,
    pub epoch: i64,
    pub config: MonitorConfig,
    pub state: State,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct State {
    pub status: Status,
    pub failure_streak: u8,
    pub first_failure_ms: Option<i64>,
    pub last_due_ms: Option<i64>,
    pub last_completed_ms: Option<i64>,
    pub last_result: Option<ProbeResult>,
    pub active_incident: Option<i64>,
    pub cadence: Cadence,
    pub next_due_ms: i64,
    pub system_error: Option<ProbeError>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    None,
    Down,
    Recovery,
}
impl State {
    pub fn new(enabled: bool, due: i64) -> Self {
        Self {
            status: if enabled {
                Status::Unknown
            } else {
                Status::Paused
            },
            failure_streak: 0,
            first_failure_ms: None,
            last_due_ms: None,
            last_completed_ms: None,
            last_result: None,
            active_incident: None,
            cadence: Cadence::Normal,
            next_due_ms: due,
            system_error: None,
        }
    }
    pub fn apply(
        &mut self,
        config: &MonitorConfig,
        due: i64,
        completed: i64,
        result: ProbeResult,
    ) -> Transition {
        let mut event = Transition::None;
        self.cadence = Cadence::Normal;
        self.system_error = if result.outcome == Outcome::Unknown {
            result.error
        } else {
            None
        };
        self.next_due_ms = due + config.interval_ms();
        match result.outcome {
            Outcome::Success => {
                self.status = Status::Up;
                self.failure_streak = 0;
                self.first_failure_ms = None;
                if self.active_incident.is_some() {
                    event = Transition::Recovery;
                }
            }
            Outcome::Failure => {
                self.first_failure_ms.get_or_insert(completed);
                self.failure_streak = (self.failure_streak + 1).min(config.failure_threshold);
                if self.active_incident.is_some() {
                    self.status = Status::Down;
                } else if self.failure_streak >= config.failure_threshold {
                    self.status = Status::Down;
                    event = Transition::Down;
                } else {
                    self.status = Status::PendingFailure;
                    self.cadence = Cadence::Confirmation;
                    self.next_due_ms = completed + CONFIRM_MS;
                }
            }
            Outcome::Unknown => {
                self.status = Status::Unknown;
                self.failure_streak = 0;
                self.first_failure_ms = None;
            }
        }
        self.last_due_ms = Some(due);
        self.last_completed_ms = Some(completed);
        self.last_result = Some(result);
        event
    }
    pub fn displayed_status(&self, config: &MonitorConfig, now: i64) -> Status {
        if !config.enabled {
            Status::Paused
        } else if self.last_completed_ms.is_none_or(|last| {
            now < last
                || now - last > 2 * config.interval_ms() + i64::from(config.deadline_seconds) * 1000
        }) {
            Status::Unknown
        } else {
            self.status
        }
    }
}
#[derive(Clone, Copy, Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Counts {
    pub expected: u64,
    pub successful: u64,
    pub failed: u64,
    pub unknown: u64,
}
impl Counts {
    pub fn availability(&self) -> Option<f64> {
        let n = self.successful + self.failed;
        (n > 0).then(|| self.successful as f64 * 100.0 / n as f64)
    }
    pub fn coverage(&self) -> Option<f64> {
        (self.expected > 0)
            .then(|| (self.successful + self.failed) as f64 * 100.0 / self.expected as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn config() -> MonitorConfig {
        MonitorConfig {
            name: "fixture".into(),
            kind: Kind::Tcp,
            target: "localhost".into(),
            port: Some(443),
            interval_seconds: 60,
            deadline_seconds: 5,
            failure_threshold: 3,
            enabled: true,
            alerts: true,
            address_family: Family::Auto,
            accepted_statuses: statuses(),
        }
    }
    #[test]
    fn confirmation_replaces_normal_due_and_success_cancels_chain() {
        let cfg = config();
        let mut state = State::new(true, 0);
        let failure = ProbeResult::fault(ProbeError::Timeout);
        assert_eq!(state.apply(&cfg, 0, 5000, failure), Transition::None);
        assert_eq!(
            (state.next_due_ms, state.cadence),
            (35000, Cadence::Confirmation)
        );
        state.apply(&cfg, 35000, 40000, failure);
        assert_eq!(state.next_due_ms, 70000);
        assert_eq!(state.apply(&cfg, 70000, 75000, failure), Transition::Down);
        assert_eq!(state.next_due_ms, 130000);
        let mut state = State::new(true, 0);
        state.apply(&cfg, 0, 5000, failure);
        state.apply(&cfg, 35000, 36000, ProbeResult::success());
        assert_eq!((state.next_due_ms, state.failure_streak), (95000, 0));
        state.apply(
            &cfg,
            95000,
            96000,
            ProbeResult::fault(ProbeError::Permission),
        );
        assert_eq!(state.status, Status::Unknown);
    }
    #[test]
    fn exact_percentages_and_confirmation_capacity() {
        let a = Counts {
            expected: 5,
            successful: 2,
            failed: 3,
            unknown: 0,
        };
        assert_eq!(a.availability(), Some(40.0));
        let a = Counts {
            expected: 5,
            successful: 3,
            failed: 1,
            unknown: 1,
        };
        assert_eq!((a.availability(), a.coverage()), (Some(75.0), Some(80.0)));
        assert_eq!(Counts::default().availability(), None);
        assert_eq!(Counts::default().coverage(), None);
        assert!((config().load() * 15.0 - 2.5).abs() < 1e-10);
        let mut cfg = config();
        cfg.failure_threshold = 1;
        assert_eq!(cfg.load() * 15.0, 1.25);
        cfg.deadline_seconds = 30;
        cfg.interval_seconds = 30;
        assert!(cfg.validate().is_err());
    }
}
