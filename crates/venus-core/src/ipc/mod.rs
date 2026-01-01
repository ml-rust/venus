//! Inter-process communication for Venus worker processes.
//!
//! This module provides the protocol and utilities for communicating
//! with isolated worker processes that execute cells.

pub mod protocol;
mod worker;

pub use protocol::{WorkerCommand, WorkerResponse, read_message, write_message};
pub use worker::{WorkerHandle, WorkerKillHandle, WorkerPool};
