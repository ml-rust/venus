//! FFI types and helpers for the venus-worker.
//!
//! Duplicated from venus-core to avoid dependency issues.

use libloading::Symbol;

use super::LoadedCell;

/// Result code from cell execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExecutionResult {
    Success = 0,
    DeserializationError = -1,
    CellError = -2,
    SerializationError = -3,
    Panic = -4,
}

impl From<i32> for ExecutionResult {
    fn from(code: i32) -> Self {
        match code {
            0 => Self::Success,
            -1 => Self::DeserializationError,
            -2 => Self::CellError,
            -3 => Self::SerializationError,
            -4 => Self::Panic,
            _ => Self::CellError,
        }
    }
}

// Entry function types - include widget_values_ptr and widget_values_len after dependencies
pub type EntryFn0 = unsafe extern "C" fn(
    *const u8, usize,  // widget_values
    *mut *mut u8, *mut usize,
) -> i32;
pub type EntryFn1 = unsafe extern "C" fn(
    *const u8, usize,  // dep 0
    *const u8, usize,  // widget_values
    *mut *mut u8, *mut usize,
) -> i32;
pub type EntryFn2 = unsafe extern "C" fn(
    *const u8, usize,  // dep 0
    *const u8, usize,  // dep 1
    *const u8, usize,  // widget_values
    *mut *mut u8, *mut usize,
) -> i32;
pub type EntryFn3 = unsafe extern "C" fn(
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,  // widget_values
    *mut *mut u8, *mut usize,
) -> i32;
pub type EntryFn4 = unsafe extern "C" fn(
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,  // widget_values
    *mut *mut u8, *mut usize,
) -> i32;
pub type EntryFn5 = unsafe extern "C" fn(
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,  // widget_values
    *mut *mut u8, *mut usize,
) -> i32;
pub type EntryFn6 = unsafe extern "C" fn(
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,  // widget_values
    *mut *mut u8, *mut usize,
) -> i32;
pub type EntryFn7 = unsafe extern "C" fn(
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,  // widget_values
    *mut *mut u8, *mut usize,
) -> i32;
pub type EntryFn8 = unsafe extern "C" fn(
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,
    *const u8, usize,  // widget_values
    *mut *mut u8, *mut usize,
) -> i32;

/// Macro to generate FFI dispatch functions for N dependencies.
macro_rules! impl_call_cell_n_deps {
    ($fn_name:ident, $fn_type:ty, $($idx:tt),*) => {
        pub fn $fn_name(
            cell: &LoadedCell,
            symbol_name: &str,
            inputs: &[Vec<u8>],
            widget_values_json: &[u8],
        ) -> Result<(Vec<u8>, Vec<u8>), String> {
            let func: Symbol<$fn_type> = unsafe { cell.library.get(symbol_name.as_bytes()) }
                .map_err(|e| format!("Failed to get symbol: {}", e))?;

            let mut out_ptr: *mut u8 = std::ptr::null_mut();
            let mut out_len: usize = 0;

            let result_code = unsafe {
                func(
                    $( inputs[$idx].as_ptr(), inputs[$idx].len(), )*
                    widget_values_json.as_ptr(), widget_values_json.len(),
                    &mut out_ptr,
                    &mut out_len,
                )
            };

            super::process_ffi_result(result_code, out_ptr, out_len, &cell.name)
        }
    };
}

impl_call_cell_n_deps!(call_cell_1_deps, EntryFn1, 0);
impl_call_cell_n_deps!(call_cell_2_deps, EntryFn2, 0, 1);
impl_call_cell_n_deps!(call_cell_3_deps, EntryFn3, 0, 1, 2);
impl_call_cell_n_deps!(call_cell_4_deps, EntryFn4, 0, 1, 2, 3);
impl_call_cell_n_deps!(call_cell_5_deps, EntryFn5, 0, 1, 2, 3, 4);
impl_call_cell_n_deps!(call_cell_6_deps, EntryFn6, 0, 1, 2, 3, 4, 5);
impl_call_cell_n_deps!(call_cell_7_deps, EntryFn7, 0, 1, 2, 3, 4, 5, 6);
impl_call_cell_n_deps!(call_cell_8_deps, EntryFn8, 0, 1, 2, 3, 4, 5, 6, 7);
