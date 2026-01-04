//! # Simple Venus Notebook
//!
//! A minimal notebook for testing the frontend.
//!
//! ## Markdown Features Demo
//!
//! This demonstrates **bold text**, *italic text*, and ***bold italic***.
//!
//! Inline code: `#[derive(Serialize, Deserialize)]` and `let x = 42;`
//!
//! Code block:
//! ```rust
//! fn example() {
//!     println!("Hello, Venus!");
//! }
//! ```
//!
//! Links: [Rust Language](https://www.rust-lang.org/) and [Venus on GitHub](https://github.com/ml-rust/venus)
//!
//! Images: ![Rust Logo](https://www.rust-lang.org/logos/rust-logo-256x256.png)

/// Types
#[derive(Debug, Clone)]
pub struct Config {
    pub name: String,
    pub count: i32,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub title: String,
    pub values: Vec<i32>,
    pub total: i32,
}

/// Configuration
#[venus::cell]
pub fn config() -> Config {
    Config {
        name: "Simple".to_string(),
        count: 15,
    }
}

// # New Markdown Cell
// 
// Edit this content...

/// Generate numbers
#[venus::cell]
pub fn numbers(config: &Config) -> Vec<i32> {
    (1..=config.count).collect()
}


/// Calculate total
#[venus::cell]
pub fn total(numbers: &Vec<i32>) -> i32 {
    numbers.iter().sum()
}


/// Generate report
#[venus::cell]
pub fn report(config: &Config, numbers: &Vec<i32>, total: &i32) -> Report {
    Report {
        title: format!("{} Report", config.name),
        values: numbers.clone(),
        total: *total,
    }
}


// # New Markdown Cell
// 
// Edit this content...
