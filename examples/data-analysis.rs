//! # Sales Data Analysis
//!
//! A realistic data analysis workflow demonstrating Venus capabilities:
//! - Data loading and cleaning
//! - Statistical computations
//! - Data filtering and aggregation
//! - Interactive parameters via widgets
//! - Custom visualization with Render trait
//!
//! This example uses basic Rust data structures. For larger datasets,
//! consider using polars or arrow for efficient DataFrame operations.

#![allow(clippy::ptr_arg)]

use std::collections::HashMap;
use venus::prelude::*;

// ### Data Structures

/// Individual sales record
#[derive(Debug, Clone, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct SalesRecord {
    pub product: String,
    pub category: String,
    pub region: String,
    pub amount: f64,
    pub quantity: i32,
    pub month: i32,
}

/// Analysis parameters configured via widgets
#[derive(Debug, Clone, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct AnalysisParams {
    pub min_amount: f64,
    pub selected_region: String,
    pub show_top_n: i32,
}

/// Statistical summary of sales data
#[derive(Debug, Clone, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct Statistics {
    pub count: usize,
    pub total_revenue: f64,
    pub total_quantity: i32,
    pub mean_amount: f64,
    pub median_amount: f64,
    pub min_amount: f64,
    pub max_amount: f64,
}

/// Aggregated sales by category
#[derive(Debug, Clone, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct CategorySummary {
    pub category: String,
    pub revenue: f64,
    pub quantity: i32,
    pub record_count: usize,
}

/// Final analysis report
#[derive(Debug, Clone, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct AnalysisReport {
    pub total_stats: Statistics,
    pub filtered_stats: Statistics,
    pub top_categories: Vec<CategorySummary>,
    pub monthly_trend: Vec<(i32, f64)>,
}

// ### Cells

/// # Analysis Parameters
///
/// Configure the analysis using interactive widgets.
/// Adjust the sliders and dropdowns to filter and customize the analysis.
#[venus::cell]
pub fn params() -> AnalysisParams {
    let min_amount = input_slider_labeled("min_amount", "Minimum Amount", 0.0, 1000.0, 50.0, 100.0);
    let selected_region = input_select_labeled(
        "region",
        "Region",
        &["All", "North", "South", "East", "West"],
        0,
    );

    let show_top_n = input_slider_labeled("top_n", "Top N Categories", 1.0, 10.0, 1.0, 5.0) as i32;

    AnalysisParams {
        min_amount,
        selected_region,
        show_top_n,
    }
}

/// # Load Sales Data
///
/// Generate sample sales data for analysis.
/// In a real scenario, this would load from CSV, database, or API.
#[venus::cell]
pub fn raw_data() -> Vec<SalesRecord> {
    vec![
        SalesRecord {
            product: "Laptop".to_string(),
            category: "Electronics".to_string(),
            region: "North".to_string(),
            amount: 1200.0,
            quantity: 2,
            month: 1,
        },
        SalesRecord {
            product: "Mouse".to_string(),
            category: "Electronics".to_string(),
            region: "South".to_string(),
            amount: 25.0,
            quantity: 10,
            month: 1,
        },
        SalesRecord {
            product: "Desk".to_string(),
            category: "Furniture".to_string(),
            region: "East".to_string(),
            amount: 450.0,
            quantity: 1,
            month: 1,
        },
        SalesRecord {
            product: "Chair".to_string(),
            category: "Furniture".to_string(),
            region: "West".to_string(),
            amount: 200.0,
            quantity: 4,
            month: 1,
        },
        SalesRecord {
            product: "Keyboard".to_string(),
            category: "Electronics".to_string(),
            region: "North".to_string(),
            amount: 75.0,
            quantity: 3,
            month: 2,
        },
        SalesRecord {
            product: "Monitor".to_string(),
            category: "Electronics".to_string(),
            region: "South".to_string(),
            amount: 300.0,
            quantity: 2,
            month: 2,
        },
        SalesRecord {
            product: "Lamp".to_string(),
            category: "Furniture".to_string(),
            region: "East".to_string(),
            amount: 50.0,
            quantity: 5,
            month: 2,
        },
        SalesRecord {
            product: "Notebook".to_string(),
            category: "Office Supplies".to_string(),
            region: "West".to_string(),
            amount: 15.0,
            quantity: 20,
            month: 2,
        },
        SalesRecord {
            product: "Pen Set".to_string(),
            category: "Office Supplies".to_string(),
            region: "North".to_string(),
            amount: 8.0,
            quantity: 30,
            month: 3,
        },
        SalesRecord {
            product: "Printer".to_string(),
            category: "Electronics".to_string(),
            region: "South".to_string(),
            amount: 350.0,
            quantity: 1,
            month: 3,
        },
        SalesRecord {
            product: "Filing Cabinet".to_string(),
            category: "Furniture".to_string(),
            region: "East".to_string(),
            amount: 280.0,
            quantity: 2,
            month: 3,
        },
        SalesRecord {
            product: "Whiteboard".to_string(),
            category: "Office Supplies".to_string(),
            region: "West".to_string(),
            amount: 120.0,
            quantity: 1,
            month: 3,
        },
    ]
}

/// # Overall Statistics
///
/// Compute statistics across all sales records.
#[venus::cell]
pub fn total_statistics(raw_data: &Vec<SalesRecord>) -> Statistics {
    if raw_data.is_empty() {
        return Statistics {
            count: 0,
            total_revenue: 0.0,
            total_quantity: 0,
            mean_amount: 0.0,
            median_amount: 0.0,
            min_amount: 0.0,
            max_amount: 0.0,
        };
    }

    let count = raw_data.len();
    let total_revenue: f64 = raw_data.iter().map(|r| r.amount).sum();
    let total_quantity: i32 = raw_data.iter().map(|r| r.quantity).sum();
    let mean_amount = total_revenue / count as f64;

    // For median, sort amounts
    let mut amounts: Vec<f64> = raw_data.iter().map(|r| r.amount).collect();
    amounts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_amount = if count % 2 == 0 {
        (amounts[count / 2 - 1] + amounts[count / 2]) / 2.0
    } else {
        amounts[count / 2]
    };

    let min_amount = amounts.first().copied().unwrap_or(0.0);
    let max_amount = amounts.last().copied().unwrap_or(0.0);

    Statistics {
        count,
        total_revenue,
        total_quantity,
        mean_amount,
        median_amount,
        min_amount,
        max_amount,
    }
}

/// # Filter Data
///
/// Apply filters based on analysis parameters.
#[venus::cell]
pub fn filtered_data(raw_data: &Vec<SalesRecord>, params: &AnalysisParams) -> Vec<SalesRecord> {
    raw_data
        .iter()
        .filter(|r| {
            // Apply amount filter
            if r.amount < params.min_amount {
                return false;
            }
            // Apply region filter
            if params.selected_region != "All" && r.region != params.selected_region {
                return false;
            }
            true
        })
        .cloned()
        .collect()
}

/// # Filtered Statistics
///
/// Compute statistics on the filtered dataset.
#[venus::cell]
pub fn filtered_statistics(filtered_data: &Vec<SalesRecord>) -> Statistics {
    if filtered_data.is_empty() {
        return Statistics {
            count: 0,
            total_revenue: 0.0,
            total_quantity: 0,
            mean_amount: 0.0,
            median_amount: 0.0,
            min_amount: 0.0,
            max_amount: 0.0,
        };
    }

    let count = filtered_data.len();
    let total_revenue: f64 = filtered_data.iter().map(|r| r.amount).sum();
    let total_quantity: i32 = filtered_data.iter().map(|r| r.quantity).sum();
    let mean_amount = total_revenue / count as f64;

    // For median, sort amounts
    let mut amounts: Vec<f64> = filtered_data.iter().map(|r| r.amount).collect();
    amounts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_amount = if count % 2 == 0 {
        (amounts[count / 2 - 1] + amounts[count / 2]) / 2.0
    } else {
        amounts[count / 2]
    };

    let min_amount = amounts.first().copied().unwrap_or(0.0);
    let max_amount = amounts.last().copied().unwrap_or(0.0);

    Statistics {
        count,
        total_revenue,
        total_quantity,
        mean_amount,
        median_amount,
        min_amount,
        max_amount,
    }
}

/// # Category Analysis
///
/// Aggregate sales by category and rank by revenue.
#[venus::cell]
pub fn category_analysis(
    filtered_data: &Vec<SalesRecord>,
    params: &AnalysisParams,
) -> Vec<CategorySummary> {
    // Aggregate by category
    let mut by_category: std::collections::HashMap<String, (f64, i32, usize)> =
        std::collections::HashMap::new();

    for record in filtered_data {
        let entry = by_category
            .entry(record.category.clone())
            .or_insert((0.0, 0, 0));
        entry.0 += record.amount;
        entry.1 += record.quantity;
        entry.2 += 1;
    }

    // Convert to summary and sort by revenue
    let mut summaries: Vec<CategorySummary> = by_category
        .into_iter()
        .map(
            |(category, (revenue, quantity, record_count))| CategorySummary {
                category,
                revenue,
                quantity,
                record_count,
            },
        )
        .collect();

    summaries.sort_by(|a, b| b.revenue.partial_cmp(&a.revenue).unwrap());

    // Take top N
    if params.show_top_n > 0 {
        summaries.truncate(params.show_top_n as usize);
    }
    summaries
}

/// # Monthly Trend
///
/// Calculate revenue trend by month.
#[venus::cell]
pub fn monthly_trend(filtered_data: &Vec<SalesRecord>) -> Vec<(i32, f64)> {
    let mut by_month: std::collections::HashMap<i32, f64> = std::collections::HashMap::new();

    for record in filtered_data {
        *by_month.entry(record.month).or_insert(0.0) += record.amount;
    }

    let mut trend: Vec<(i32, f64)> = by_month.into_iter().collect();
    trend.sort_by_key(|(month, _)| *month);
    trend
}

/// # Analysis Report
///
/// Combine all analysis results into a comprehensive report.
#[venus::cell]
pub fn report(
    total_statistics: &Statistics,
    filtered_statistics: &Statistics,
    category_analysis: &Vec<CategorySummary>,
    monthly_trend: &Vec<(i32, f64)>,
) -> AnalysisReport {
    AnalysisReport {
        total_stats: total_statistics.clone(),
        filtered_stats: filtered_statistics.clone(),
        top_categories: category_analysis.clone(),
        monthly_trend: monthly_trend.clone(),
    }
}

// Custom Render Implementations
#[venus::hide]
impl Render for Statistics {
    fn render_text(&self) -> String {
        format!(
            "Records: {}\n\
             Total Revenue: ${:.2}\n\
             Total Quantity: {}\n\
             Mean: ${:.2}\n\
             Median: ${:.2}\n\
             Range: ${:.2} - ${:.2}",
            self.count,
            self.total_revenue,
            self.total_quantity,
            self.mean_amount,
            self.median_amount,
            self.min_amount,
            self.max_amount
        )
    }

    fn render_html(&self) -> Option<String> {
        Some(format!(
            "<div style='font-family: monospace; background: #f5f5f5; padding: 15px; border-radius: 5px;'>\
                <table style='border-collapse: collapse;'>\
                    <tr><td style='padding: 5px;'><b>Records:</b></td><td style='padding: 5px;'>{}</td></tr>\
                    <tr><td style='padding: 5px;'><b>Total Revenue:</b></td><td style='padding: 5px;'>${:.2}</td></tr>\
                    <tr><td style='padding: 5px;'><b>Total Quantity:</b></td><td style='padding: 5px;'>{}</td></tr>\
                    <tr><td style='padding: 5px;'><b>Mean Amount:</b></td><td style='padding: 5px;'>${:.2}</td></tr>\
                    <tr><td style='padding: 5px;'><b>Median Amount:</b></td><td style='padding: 5px;'>${:.2}</td></tr>\
                    <tr><td style='padding: 5px;'><b>Range:</b></td><td style='padding: 5px;'>${:.2} - ${:.2}</td></tr>\
                </table>\
            </div>",
            self.count,
            self.total_revenue,
            self.total_quantity,
            self.mean_amount,
            self.median_amount,
            self.min_amount,
            self.max_amount
        ))
    }
}

#[venus::hide]
impl Render for AnalysisReport {
    fn render_text(&self) -> String {
        let mut output = String::new();

        output.push_str("=== SALES ANALYSIS REPORT ===\n\n");

        output.push_str("Overall Statistics:\n");
        output.push_str(&self.total_stats.render_text());
        output.push_str("\n\n");

        output.push_str("Filtered Statistics:\n");
        output.push_str(&self.filtered_stats.render_text());
        output.push_str("\n\n");

        output.push_str("Top Categories by Revenue:\n");
        for (i, cat) in self.top_categories.iter().enumerate() {
            output.push_str(&format!(
                "{}. {} - ${:.2} ({} items, {} records)\n",
                i + 1,
                cat.category,
                cat.revenue,
                cat.quantity,
                cat.record_count
            ));
        }
        output.push_str("\n");

        output.push_str("Monthly Revenue Trend:\n");
        for (month, revenue) in &self.monthly_trend {
            output.push_str(&format!("Month {}: ${:.2}\n", month, revenue));
        }

        output
    }

    fn render_html(&self) -> Option<String> {
        let mut html = String::new();

        html.push_str("<div style='font-family: Arial, sans-serif;'>");
        html.push_str("<h2 style='color: #2c3e50;'>📊 Sales Analysis Report</h2>");

        // Overall stats
        html.push_str("<h3 style='color: #34495e;'>Overall Statistics</h3>");
        html.push_str(&self.total_stats.render_html().unwrap_or_default());

        // Filtered stats
        html.push_str("<h3 style='color: #34495e; margin-top: 20px;'>Filtered Statistics</h3>");
        html.push_str(&self.filtered_stats.render_html().unwrap_or_default());

        // Top categories
        html.push_str(
            "<h3 style='color: #34495e; margin-top: 20px;'>Top Categories by Revenue</h3>",
        );
        html.push_str("<table style='border-collapse: collapse; width: 100%; margin-top: 10px;'>");
        html.push_str(
            "<tr style='background: #3498db; color: white;'>\
            <th style='padding: 10px; text-align: left;'>Rank</th>\
            <th style='padding: 10px; text-align: left;'>Category</th>\
            <th style='padding: 10px; text-align: right;'>Revenue</th>\
            <th style='padding: 10px; text-align: right;'>Quantity</th>\
            <th style='padding: 10px; text-align: right;'>Records</th>\
        </tr>",
        );

        for (i, cat) in self.top_categories.iter().enumerate() {
            let bg = if i % 2 == 0 { "#ecf0f1" } else { "white" };
            html.push_str(&format!(
                "<tr style='background: {};'>\
                    <td style='padding: 8px;'>{}</td>\
                    <td style='padding: 8px;'>{}</td>\
                    <td style='padding: 8px; text-align: right;'>${:.2}</td>\
                    <td style='padding: 8px; text-align: right;'>{}</td>\
                    <td style='padding: 8px; text-align: right;'>{}</td>\
                </tr>",
                bg,
                i + 1,
                cat.category,
                cat.revenue,
                cat.quantity,
                cat.record_count
            ));
        }
        html.push_str("</table>");

        // Monthly trend
        html.push_str("<h3 style='color: #34495e; margin-top: 20px;'>Monthly Revenue Trend</h3>");
        html.push_str("<div style='background: #f5f5f5; padding: 15px; border-radius: 5px;'>");
        for (month, revenue) in &self.monthly_trend {
            let bar_width = (revenue
                / self
                    .monthly_trend
                    .iter()
                    .map(|(_, r)| r)
                    .fold(0.0_f64, |a, &b| a.max(b))
                * 300.0) as i32;
            html.push_str(&format!(
                "<div style='margin: 5px 0;'>\
                    <span style='display: inline-block; width: 80px;'>Month {}:</span>\
                    <span style='display: inline-block; width: {}px; height: 20px; background: #3498db; margin-right: 10px;'></span>\
                    <span>${:.2}</span>\
                </div>",
                month, bar_width, revenue
            ));
        }
        html.push_str("</div>");

        html.push_str("</div>");

        Some(html)
    }
}
