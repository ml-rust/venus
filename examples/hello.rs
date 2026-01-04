//! # Hello World Notebook
//!
//! A simple Venus notebook demonstrating basic cell functionality.
//!

#![allow(clippy::ptr_arg)]

use venus::prelude::*;

#[derive(Debug, Clone, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct Config {
    pub name: String,
    pub iterations: i32,
}

#[derive(Debug, Clone, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct Summary {
    pub message: String,
    pub values: Vec<i32>,
    pub total: i32,
}

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

/// New cell
#[venus::cell]
pub fn new_cell_1() -> String {
    "Hello".to_string()
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
