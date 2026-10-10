use std::fmt;

/// Safe categories only. Raw SQL, paths, network errors and configuration values
/// must never enter client responses or the process's diagnostic output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Configuration,
    Io,
    AlreadyOwned,
    Storage,
    Schema,
    DiskPressure,
    Busy,
    Unavailable,
    Deadline,
    WorkerStopped,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn code(self) -> &'static str {
        match self {
            Self::Configuration => "invalid_configuration",
            Self::Io => "filesystem_unavailable",
            Self::AlreadyOwned => "state_already_owned",
            Self::Storage => "storage_failure",
            Self::Schema => "incompatible_schema",
            Self::DiskPressure => "disk_pressure",
            Self::Busy => "capacity_busy",
            Self::Unavailable => "not_ready",
            Self::Deadline => "deadline_exceeded",
            Self::WorkerStopped => "worker_stopped",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for Error {}
