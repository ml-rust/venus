//! Universe library - compiled with LLVM, provides shared types and functions.
//!
//! This simulates the "Universe" dylib that contains pre-compiled dependencies.

#[repr(C)]
#[derive(Debug, Clone)]
pub struct DataFrame {
    pub rows: usize,
    pub cols: usize,
    pub name: String,
}

impl DataFrame {
    pub fn new(name: &str, rows: usize, cols: usize) -> Self {
        Self {
            rows,
            cols,
            name: name.to_string(),
        }
    }

    pub fn filter(&self, predicate: fn(usize) -> bool) -> Self {
        let filtered_rows = (0..self.rows).filter(|&r| predicate(r)).count();
        Self {
            rows: filtered_rows,
            cols: self.cols,
            name: format!("{}_filtered", self.name),
        }
    }
}

/// Export function that cells can call
#[no_mangle]
pub extern "C" fn universe_create_dataframe(name_ptr: *const u8, name_len: usize, rows: usize, cols: usize) -> *mut DataFrame {
    let name = unsafe {
        let slice = std::slice::from_raw_parts(name_ptr, name_len);
        std::str::from_utf8_unchecked(slice)
    };
    Box::into_raw(Box::new(DataFrame::new(name, rows, cols)))
}

#[no_mangle]
pub extern "C" fn universe_free_dataframe(df: *mut DataFrame) {
    if !df.is_null() {
        unsafe { drop(Box::from_raw(df)); }
    }
}

#[no_mangle]
pub extern "C" fn universe_dataframe_rows(df: *const DataFrame) -> usize {
    if df.is_null() { 0 } else { unsafe { (*df).rows } }
}
