//! # Simple Venus Notebook
//!
//! A minimal notebook for testing the frontend.
//!
//! Note: rkyv serialization is automatically included by Venus universe.
//! Use `#[derive(Serialize, Deserialize)]` and Venus will transform to rkyv.

#![allow(clippy::ptr_arg)]

/// # Configuration
///
/// Basic configuration cell.
#[venus::cell]
pub fn config() -> Config {
    Config {
        name: "Venus Test".to_string(),
        count: 5,
    }
}

/// # Numbers
///
/// Generate a sequence of squared numbers.
#[venus::cell]
pub fn numbers(config: &Config) -> Vec<i32> {
    (1..=config.count).map(|i| i * i).collect()
}

/// # Sum
///
/// Calculate the sum of all numbers.
#[venus::cell]
pub fn sum(numbers: &Vec<i32>) -> i32 {
    numbers.iter().sum()
}

/// # Report
///
/// Generate a final report combining all results.
#[venus::cell]
pub fn report(config: &Config, numbers: &Vec<i32>, sum: &i32) -> Report {
    Report {
        title: config.name.clone(),
        values: numbers.clone(),
        total: *sum,
    }
}

// Types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub name: String,
    pub count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub title: String,
    pub values: Vec<i32>,
    pub total: i32,
}

fn main() {
    println!("=== Simple Venus Notebook ===\n");

    let cfg = config();
    println!("Config: {:?}", cfg);

    let nums = numbers(&cfg);
    println!("Numbers: {:?}", nums);

    let total = sum(&nums);
    println!("Sum: {}", total);

    let rpt = report(&cfg, &nums, &total);
    println!("Report: {:?}", rpt);
}
