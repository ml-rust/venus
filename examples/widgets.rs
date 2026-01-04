//! # Interactive Widgets Demo
//!
//! Demonstrates Venus interactive widgets for dynamic notebook parameters.
//!
//! Widgets allow users to adjust values through UI controls (sliders, text inputs,
//! checkboxes, dropdowns) and re-run cells to see updated results.

#![allow(clippy::ptr_arg)]

use venus::prelude::*;

// Types
#[derive(Debug, Clone, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct Params {
    pub count: i32,
    pub multiplier: f64,
    pub label: String,
    pub include_squares: bool,
    pub mode: String,
}

#[derive(Debug, Clone, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct Report {
    pub title: String,
    pub mode: String,
    pub count: i32,
    pub multiplier: f64,
    pub include_squares: bool,
    pub values: Vec<f64>,
    pub final_result: f64,
}

/// # Parameters
///
/// Configure the computation parameters using interactive widgets.
#[venus::cell]
pub fn params() -> Params {
    // Sliders for numeric values
    let count = input_slider("count", 1.0, 20.0, 10.0) as i32;
    let multiplier = input_slider("multiplier", 0.1, 5.0, 1.0);

    // Text input for custom label
    let label = input_text_with_default("label", "Enter title...", "Results");

    // Checkbox for options
    let include_squares = input_checkbox("squares", true);

    // Dropdown for mode selection (default index 0 = "sum")
    let mode = input_select("mode", &["sum", "product", "average"], 0);

    Params {
        count,
        multiplier,
        label,
        include_squares,
        mode,
    }
}

/// # Generate Numbers
///
/// Generate a sequence based on parameters.
#[venus::cell]
pub fn numbers(params: &Params) -> Vec<f64> {
    let base: Vec<i32> = (1..=params.count).collect();

    base.iter()
        .map(|&n| {
            let val = if params.include_squares {
                (n * n) as f64
            } else {
                n as f64
            };
            val * params.multiplier
        })
        .collect()
}

/// # Compute Result
///
/// Apply the selected operation mode.
#[venus::cell]
pub fn result(params: &Params, numbers: &Vec<f64>) -> f64 {
    match params.mode.as_str() {
        "sum" => numbers.iter().sum(),
        "product" => numbers.iter().product(),
        "average" => {
            if numbers.is_empty() {
                0.0
            } else {
                numbers.iter().sum::<f64>() / numbers.len() as f64
            }
        }
        _ => 0.0,
    }
}

/// # Report
///
/// Generate a formatted report with all results.
#[venus::cell]
pub fn report(params: &Params, numbers: &Vec<f64>, result: &f64) -> Report {
    Report {
        title: params.label.clone(),
        mode: params.mode.clone(),
        count: params.count,
        multiplier: params.multiplier,
        include_squares: params.include_squares,
        values: numbers.clone(),
        final_result: *result,
    }
}
