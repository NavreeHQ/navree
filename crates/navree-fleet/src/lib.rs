//! # navree-fleet
#![forbid(unsafe_code)]

mod balance;
mod error;
mod options;
mod workload;

pub use balance::{Allocation, initial_allocation, next_allocation, target_allocation};
pub use error::FleetError;
pub use options::{
    Balance, DEFAULT_SUPERVISOR, FleetOptions, Supervisor, SupervisorOptions, TrimOptions,
};
pub use workload::{ASSUMED_RUNTIME_MS, QueueWorkload};
