//! Output value decoding from rkyv bytes.
//!
//! Decodes serialized cell outputs back to displayable strings
//! based on the cell's return type.

/// Decode a primitive type from bytes and format it.
macro_rules! decode_primitive {
    ($bytes:expr, $type:ty) => {
        rkyv::from_bytes::<$type, rkyv::rancor::Error>($bytes)
            .ok()
            .map(|v| format!("{}", v))
    };
    ($bytes:expr, $type:ty, debug) => {
        rkyv::from_bytes::<$type, rkyv::rancor::Error>($bytes)
            .ok()
            .map(|v| format!("{:?}", v))
    };
    ($bytes:expr, $type:ty, quoted) => {
        rkyv::from_bytes::<$type, rkyv::rancor::Error>($bytes)
            .ok()
            .map(|v| format!("\"{}\"", v))
    };
}

/// Try to decode a value from bytes based on type name.
///
/// Returns `Some(formatted_string)` if the type is recognized and decoding succeeds,
/// `None` otherwise.
///
/// # Supported Types
///
/// - Primitives: i8, i16, i32, i64, u8, u16, u32, u64, f32, f64, bool
/// - Strings: String, &str
/// - Unit: ()
/// - Vectors: Vec<i32>, Vec<i64>, Vec<f32>, Vec<f64>, Vec<String>
/// - Options: Option<i32>, Option<String>
///
/// # Arguments
///
/// * `type_name` - The Rust type name as a string (e.g., "i32", "Vec<String>")
/// * `bytes` - The rkyv-serialized bytes
pub fn try_decode_value(type_name: &str, bytes: &[u8]) -> Option<String> {
    match type_name {
        // Primitives
        "i8" => decode_primitive!(bytes, i8),
        "i16" => decode_primitive!(bytes, i16),
        "i32" => decode_primitive!(bytes, i32),
        "i64" => decode_primitive!(bytes, i64),
        "u8" => decode_primitive!(bytes, u8),
        "u16" => decode_primitive!(bytes, u16),
        "u32" => decode_primitive!(bytes, u32),
        "u64" => decode_primitive!(bytes, u64),
        "f32" => decode_primitive!(bytes, f32),
        "f64" => decode_primitive!(bytes, f64),
        "bool" => decode_primitive!(bytes, bool),
        "String" | "&str" => decode_primitive!(bytes, String, quoted),
        "()" => Some("()".to_string()),

        // Vec types
        t if t.starts_with("Vec<") => decode_vec(t, bytes),

        // Option types
        t if t.starts_with("Option<") => decode_option(t, bytes),

        // Unsupported type
        _ => None,
    }
}

/// Decode Vec types.
fn decode_vec(type_name: &str, bytes: &[u8]) -> Option<String> {
    let inner = type_name.strip_prefix("Vec<")?.strip_suffix('>')?;
    match inner {
        "i32" => decode_primitive!(bytes, Vec<i32>, debug),
        "i64" => decode_primitive!(bytes, Vec<i64>, debug),
        "f32" => decode_primitive!(bytes, Vec<f32>, debug),
        "f64" => decode_primitive!(bytes, Vec<f64>, debug),
        "u8" => decode_primitive!(bytes, Vec<u8>, debug),
        "u16" => decode_primitive!(bytes, Vec<u16>, debug),
        "u32" => decode_primitive!(bytes, Vec<u32>, debug),
        "u64" => decode_primitive!(bytes, Vec<u64>, debug),
        "String" => decode_primitive!(bytes, Vec<String>, debug),
        "bool" => decode_primitive!(bytes, Vec<bool>, debug),
        _ => None,
    }
}

/// Decode Option types.
fn decode_option(type_name: &str, bytes: &[u8]) -> Option<String> {
    let inner = type_name.strip_prefix("Option<")?.strip_suffix('>')?;
    match inner {
        "i32" => decode_primitive!(bytes, Option<i32>, debug),
        "i64" => decode_primitive!(bytes, Option<i64>, debug),
        "f32" => decode_primitive!(bytes, Option<f32>, debug),
        "f64" => decode_primitive!(bytes, Option<f64>, debug),
        "String" => decode_primitive!(bytes, Option<String>, debug),
        "bool" => decode_primitive!(bytes, Option<bool>, debug),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_i32() {
        let value: i32 = 42;
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&value).unwrap();
        assert_eq!(try_decode_value("i32", &bytes), Some("42".to_string()));
    }

    #[test]
    fn test_decode_string() {
        let value = "hello".to_string();
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&value).unwrap();
        assert_eq!(
            try_decode_value("String", &bytes),
            Some("\"hello\"".to_string())
        );
    }

    #[test]
    fn test_decode_vec_i32() {
        let value: Vec<i32> = vec![1, 2, 3];
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&value).unwrap();
        assert_eq!(
            try_decode_value("Vec<i32>", &bytes),
            Some("[1, 2, 3]".to_string())
        );
    }

    #[test]
    fn test_decode_unknown_type() {
        assert_eq!(try_decode_value("CustomType", &[1, 2, 3]), None);
    }

    #[test]
    fn test_decode_unit() {
        assert_eq!(try_decode_value("()", &[]), Some("()".to_string()));
    }
}
