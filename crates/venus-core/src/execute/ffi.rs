//! FFI type definitions for cell execution.
//!
//! This module defines the function pointer types used to call
//! compiled cell entry points, and the result codes they return.

/// Result code from cell execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExecutionResult {
    /// Cell executed successfully
    Success = 0,
    /// Failed to deserialize input
    DeserializationError = -1,
    /// Cell function returned an error
    CellError = -2,
    /// Failed to serialize output
    SerializationError = -3,
    /// Cell panicked during execution
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
            _ => Self::CellError, // Unknown codes treated as cell errors
        }
    }
}

// =============================================================================
// FFI Entry Function Types
// =============================================================================
//
// Type aliases for FFI entry functions with N inputs.
// Each input requires (ptr, len) pairs, followed by widget_values (ptr, len),
// then output params (out_ptr, out_len).
// All functions return i32 status code and write output to (out_ptr, out_len).

/// Entry function for cells with 0 dependencies.
pub type EntryFn0 = unsafe extern "C" fn(
    *const u8, usize,  // widget_values
    *mut *mut u8, *mut usize,
) -> i32;

/// Entry function for cells with 1 dependency.
pub type EntryFn1 = unsafe extern "C" fn(
    *const u8, usize,  // dep 0
    *const u8, usize,  // widget_values
    *mut *mut u8, *mut usize,
) -> i32;

/// Entry function for cells with 2 dependencies.
pub type EntryFn2 = unsafe extern "C" fn(
    *const u8, usize,  // dep 0
    *const u8, usize,  // dep 1
    *const u8, usize,  // widget_values
    *mut *mut u8, *mut usize,
) -> i32;

/// Entry function for cells with 3 dependencies.
pub type EntryFn3 = unsafe extern "C" fn(
    *const u8, usize,  // dep 0
    *const u8, usize,  // dep 1
    *const u8, usize,  // dep 2
    *const u8, usize,  // widget_values
    *mut *mut u8, *mut usize,
) -> i32;

/// Entry function for cells with 4 dependencies.
pub type EntryFn4 = unsafe extern "C" fn(
    *const u8, usize,  // dep 0
    *const u8, usize,  // dep 1
    *const u8, usize,  // dep 2
    *const u8, usize,  // dep 3
    *const u8, usize,  // widget_values
    *mut *mut u8, *mut usize,
) -> i32;

/// Entry function for cells with 5 dependencies.
pub type EntryFn5 = unsafe extern "C" fn(
    *const u8, usize,  // dep 0
    *const u8, usize,  // dep 1
    *const u8, usize,  // dep 2
    *const u8, usize,  // dep 3
    *const u8, usize,  // dep 4
    *const u8, usize,  // widget_values
    *mut *mut u8, *mut usize,
) -> i32;

/// Entry function for cells with 6 dependencies.
pub type EntryFn6 = unsafe extern "C" fn(
    *const u8, usize,  // dep 0
    *const u8, usize,  // dep 1
    *const u8, usize,  // dep 2
    *const u8, usize,  // dep 3
    *const u8, usize,  // dep 4
    *const u8, usize,  // dep 5
    *const u8, usize,  // widget_values
    *mut *mut u8, *mut usize,
) -> i32;

/// Entry function for cells with 7 dependencies.
pub type EntryFn7 = unsafe extern "C" fn(
    *const u8, usize,  // dep 0
    *const u8, usize,  // dep 1
    *const u8, usize,  // dep 2
    *const u8, usize,  // dep 3
    *const u8, usize,  // dep 4
    *const u8, usize,  // dep 5
    *const u8, usize,  // dep 6
    *const u8, usize,  // widget_values
    *mut *mut u8, *mut usize,
) -> i32;

/// Entry function for cells with 8 dependencies.
pub type EntryFn8 = unsafe extern "C" fn(
    *const u8, usize,  // dep 0
    *const u8, usize,  // dep 1
    *const u8, usize,  // dep 2
    *const u8, usize,  // dep 3
    *const u8, usize,  // dep 4
    *const u8, usize,  // dep 5
    *const u8, usize,  // dep 6
    *const u8, usize,  // dep 7
    *const u8, usize,  // widget_values
    *mut *mut u8, *mut usize,
) -> i32;

// =============================================================================
// FFI Dispatch Macro
// =============================================================================

/// Macro to generate FFI dispatch for cells with N dependencies.
///
/// This eliminates duplication across individual call functions.
/// Each invocation generates a typed FFI call with the appropriate entry function type.
///
/// # Safety
/// The generated code trusts that the symbol has the correct signature.
///
/// # Usage
/// ```ignore
/// call_cell_n_deps!(self, loaded, symbol_name, inputs, widget_values, EntryFn2, 0, 1)
/// ```
macro_rules! call_cell_n_deps {
    ($executor:expr, $loaded:expr, $symbol_name:expr, $inputs:expr, $widget_values:expr, $fn_type:ty, $($idx:tt),*) => {{
        use libloading::Symbol;
        use $crate::error::Error;

        let func: Symbol<$fn_type> = unsafe {
            $loaded.library.get($symbol_name.as_bytes())
        }.map_err(|e| {
            Error::Execution(format!("Failed to get symbol {}: {}", $symbol_name, e))
        })?;

        let mut out_ptr: *mut u8 = std::ptr::null_mut();
        let mut out_len: usize = 0;

        // Extract input byte slices
        $( let _input_bytes = $inputs[$idx].bytes(); )*
        let inputs_array = [$( $inputs[$idx].bytes() ),*];

        // Call the FFI function with widget_values
        let result_code = unsafe {
            func(
                $( inputs_array[$idx].as_ptr(), inputs_array[$idx].len(), )*
                $widget_values.as_ptr(), $widget_values.len(),
                &mut out_ptr,
                &mut out_len,
            )
        };

        $executor.process_ffi_result(result_code, out_ptr, out_len, &$loaded.compiled.name)
    }};
}

pub(crate) use call_cell_n_deps;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_result_from_i32() {
        assert_eq!(ExecutionResult::from(0), ExecutionResult::Success);
        assert_eq!(
            ExecutionResult::from(-1),
            ExecutionResult::DeserializationError
        );
        assert_eq!(ExecutionResult::from(-2), ExecutionResult::CellError);
        assert_eq!(
            ExecutionResult::from(-3),
            ExecutionResult::SerializationError
        );
        assert_eq!(ExecutionResult::from(-4), ExecutionResult::Panic);
        assert_eq!(ExecutionResult::from(-99), ExecutionResult::CellError);
    }
}
