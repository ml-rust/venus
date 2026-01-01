//! Cell library - compiled with Cranelift, uses Universe functions.
//!
//! This simulates a notebook cell compiled with fast Cranelift backend.

// FFI declarations for Universe functions
extern "C" {
    fn universe_create_dataframe(name_ptr: *const u8, name_len: usize, rows: usize, cols: usize) -> *mut ();
    fn universe_free_dataframe(df: *mut ());
    fn universe_dataframe_rows(df: *const ()) -> usize;
}

/// Cell entry point - called by Venus runtime
#[no_mangle]
pub extern "C" fn cell_execute() -> usize {
    let name = "test_data";

    unsafe {
        // Create a DataFrame using Universe (LLVM-compiled)
        let df = universe_create_dataframe(
            name.as_ptr(),
            name.len(),
            1000,
            10
        );

        // Read data from it
        let rows = universe_dataframe_rows(df);

        // Clean up
        universe_free_dataframe(df);

        // Return result
        rows
    }
}

/// Simple computation that doesn't use Universe
#[no_mangle]
pub extern "C" fn cell_compute(a: i64, b: i64) -> i64 {
    a + b * 2
}
