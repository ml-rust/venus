//! # Hello World Notebook
//!
//! A simple Venus notebook demonstrating basic cell functionality.
//!
//! ```cargo
//! [dependencies]
//! venus = { path = "../crates/venus" }
//! ```

// Venus cells use &String/&Vec<T> rather than &str/&[T] because
// dependency resolution matches parameter types to producer return types exactly.
#![allow(clippy::ptr_arg)]

use venus::prelude::*;

/// # Configuration
///
/// This cell provides configuration for the notebook.
#[venus::cell]
pub fn config() -> Config {
    Config {
        name: "Hello Venus".to_string(),
        iterations: 10,
    }
}

/// # Greeting
///
/// Generate a greeting message using the config.
#[venus::cell]
pub fn greeting(config: &Config) -> String {
    format!("Hello from {}!", config.name)
}

/// # Computation
///
/// Perform a simple computation based on config.
#[venus::cell]
pub fn compute(config: &Config) -> Vec<i32> {
    (0..config.iterations).map(|i| i * i).collect()
}

/// # Result
///
/// Combine greeting and computation results.
#[venus::cell]
pub fn result(greeting: &String, compute: &Vec<i32>) -> Summary {
    Summary {
        message: greeting.clone(),
        values: compute.clone(),
        total: compute.iter().sum(),
    }
}

// Supporting types

#[derive(Debug, Clone)]
pub struct Config {
    pub name: String,
    pub iterations: i32,
}

#[derive(Debug, Clone)]
pub struct Summary {
    pub message: String,
    pub values: Vec<i32>,
    pub total: i32,
}

impl Render for Summary {
    fn render_text(&self) -> String {
        format!(
            "{}\nValues: {:?}\nTotal: {}",
            self.message, self.values, self.total
        )
    }

    fn render_html(&self) -> Option<String> {
        Some(format!(
            "<div class='summary'>\
                <h3>{}</h3>\
                <p>Values: {:?}</p>\
                <p><strong>Total: {}</strong></p>\
            </div>",
            self.message, self.values, self.total
        ))
    }
}

// When run as a standalone binary, execute all cells in order
fn main() {
    println!("=== Venus Notebook: Hello World ===\n");

    // Execute cells in dependency order
    let cfg = config();
    println!("Config: {:?}\n", cfg);

    let greet = greeting(&cfg);
    println!("Greeting: {}\n", greet);

    let values = compute(&cfg);
    println!("Compute: {:?}\n", values);

    let summary = result(&greet, &values);
    println!("Result:\n{}", summary.render_text());
}
