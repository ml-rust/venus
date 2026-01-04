//! Test notebook for process isolation - contains an infinite loop.
//!
//! This cell will run forever unless killed via process isolation.

use venus::prelude::*;

/// This cell loops forever - tests process isolation interrupt.
#[venus::cell]
pub fn infinite_loop() -> i32 {
    let mut i = 0i32;
    loop {
        i = i.wrapping_add(1);
        // Busy loop - use black_box to prevent any optimization
        std::hint::black_box(i);
        // This condition is never true, loop runs forever
        if i == i32::MIN && i == i32::MAX {
            break;
        }
    }
    i
}