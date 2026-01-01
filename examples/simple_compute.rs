//! Test notebook for process isolation - simple computation.
//!
//! This cell performs a quick computation and returns a result.

//! [dependencies]
//! # No dependencies needed

use venus::prelude::*;

/// Simple computation cell that returns quickly.
#[venus::cell]
pub fn simple_compute() -> i32 {
    let mut sum = 0i32;
    for i in 0..1000 {
        sum = sum.wrapping_add(i);
    }
    sum
}
