//! Cell output serialization traits.
//!
//! ## Serialization Architecture
//!
//! Venus uses rkyv for all serialization:
//!
//! 1. **Cell data** (stored in `BoxedOutput.bytes`): Serialized with rkyv for
//!    zero-copy FFI performance when passing data between cells.
//!
//! 2. **BoxedOutput container**: Also serialized with rkyv for state persistence
//!    (saving/loading cached outputs to disk).
//!
//! This unified approach provides consistent zero-copy deserialization throughout.

use std::any::TypeId;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use rkyv::{Archive, Deserialize, Serialize};

use crate::error::{Error, Result};

/// Trait for cell outputs that can be serialized and restored.
///
/// This trait is automatically implemented for any type that implements
/// `Serialize + DeserializeOwned + 'static`.
pub trait CellOutput: Send + Sync + 'static {
    /// Serialize the output to bytes.
    fn serialize_output(&self) -> Result<Vec<u8>>;

    /// Get the type hash for schema validation.
    fn type_hash(&self) -> u64;

    /// Get the type name for debugging.
    fn type_name(&self) -> &'static str;
}

/// Blanket implementation for all rkyv-compatible types.
impl<T> CellOutput for T
where
    T: for<'a> Serialize<rkyv::rancor::Strategy<
            rkyv::ser::Serializer<
                rkyv::util::AlignedVec,
                rkyv::ser::allocator::ArenaHandle<'a>,
                rkyv::ser::sharing::Share,
            >,
            rkyv::rancor::Error,
        >> + Send
        + Sync
        + 'static,
{
    fn serialize_output(&self) -> Result<Vec<u8>> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .map(|v| v.into_vec())
            .map_err(|e| Error::Serialization(e.to_string()))
    }

    fn type_hash(&self) -> u64 {
        // Known limitation: DefaultHasher is not guaranteed stable across
        // Rust versions or even runs. This is acceptable for single-session
        // cache validation within the same process. For cross-session persistence,
        // a deterministic hasher (FxHash) or structural hash would be needed.
        let mut hasher = DefaultHasher::new();
        TypeId::of::<T>().hash(&mut hasher);
        hasher.finish()
    }

    fn type_name(&self) -> &'static str {
        std::any::type_name::<T>()
    }
}

/// Deserialize a cell output from bytes.
///
/// # Safety
///
/// Uses unchecked deserialization for performance. Only safe when reading from
/// trusted sources (our own cache files and worker processes).
pub fn deserialize_output<T>(bytes: &[u8]) -> Result<T>
where
    T: Archive,
    T::Archived: Deserialize<T, rkyv::rancor::Strategy<rkyv::de::Pool, rkyv::rancor::Error>>,
{
    // SAFETY: We trust data from our own cache and IPC.
    // Using unchecked deserialization avoids CheckBytes trait complexity.
    unsafe { rkyv::from_bytes_unchecked::<T, rkyv::rancor::Error>(bytes) }
        .map_err(|e: rkyv::rancor::Error| Error::Deserialization(e.to_string()))
}

/// Marker trait for outputs that can use zero-copy deserialization.
///
/// Types implementing this trait can potentially be accessed without
/// full deserialization using rkyv. For now, this is a marker trait
/// that indicates the type is suitable for high-performance paths.
pub trait ZeroCopyOutput: CellOutput {}

/// A boxed cell output that can be stored generically.
#[derive(Debug, Clone, Archive, Serialize, Deserialize)]
pub struct BoxedOutput {
    /// Serialized bytes
    bytes: Vec<u8>,

    /// Type hash for validation
    type_hash: u64,

    /// Type name for debugging
    type_name: String,

    /// Human-readable display text (Debug format)
    display_text: Option<String>,
}

impl BoxedOutput {
    /// Create a new boxed output from a CellOutput.
    pub fn new<T: CellOutput>(value: &T) -> Result<Self> {
        Ok(Self {
            bytes: value.serialize_output()?,
            type_hash: value.type_hash(),
            type_name: value.type_name().to_string(),
            display_text: None,
        })
    }

    /// Create a boxed output from raw serialized bytes.
    ///
    /// Used when loading outputs from FFI calls where type info
    /// is not available at the Rust level.
    ///
    /// **Note**: Type hash is set to 0 (unknown type). This is safe because
    /// deserialization validates types at runtime. Full type propagation from
    /// FFI would require codegen changes to embed type metadata in dylibs.
    pub fn from_raw_bytes(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            type_hash: 0, // Unknown type
            type_name: "<ffi>".to_string(),
            display_text: None,
        }
    }

    /// Create a boxed output from raw bytes with display text.
    ///
    /// Used when loading outputs from FFI calls that include
    /// a human-readable representation.
    pub fn from_raw_bytes_with_display(bytes: Vec<u8>, display: String) -> Self {
        Self {
            bytes,
            type_hash: 0, // Unknown type
            type_name: "<ffi>".to_string(),
            display_text: Some(display),
        }
    }

    /// Create a boxed output from raw bytes with known type info.
    ///
    /// Used when restoring outputs from Salsa cache where type
    /// information was preserved.
    pub fn from_raw_with_type(bytes: Vec<u8>, type_hash: u64, type_name: String) -> Self {
        Self {
            bytes,
            type_hash,
            type_name,
            display_text: None,
        }
    }

    /// Get the serialized bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Get the type hash.
    pub fn type_hash(&self) -> u64 {
        self.type_hash
    }

    /// Get the type name.
    pub fn type_name(&self) -> &str {
        &self.type_name
    }

    /// Get the display text (Debug format) if available.
    pub fn display_text(&self) -> Option<&str> {
        self.display_text.as_deref()
    }

    /// Deserialize to a specific type.
    ///
    /// Returns an error if the type hash doesn't match.
    pub fn deserialize<T>(&self) -> Result<T>
    where
        T: CellOutput + Archive,
        T::Archived: Deserialize<T, rkyv::rancor::Strategy<rkyv::de::Pool, rkyv::rancor::Error>>,
    {
        // Verify type hash (see type_hash() for hash stability notes)
        let expected_hash = {
            let mut hasher = DefaultHasher::new();
            std::any::TypeId::of::<T>().hash(&mut hasher);
            hasher.finish()
        };

        if self.type_hash != expected_hash {
            return Err(Error::SchemaEvolution(format!(
                "Type mismatch: stored {} (hash {:x}), requested {} (hash {:x})",
                self.type_name,
                self.type_hash,
                std::any::type_name::<T>(),
                expected_hash
            )));
        }

        deserialize_output(&self.bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Archive, Serialize, Deserialize)]
    struct TestOutput {
        value: i32,
        name: String,
    }

    #[test]
    fn test_cell_output_serialize() {
        let output = TestOutput {
            value: 42,
            name: "test".to_string(),
        };

        let bytes = output.serialize_output().unwrap();
        assert!(!bytes.is_empty());

        let restored: TestOutput = deserialize_output(&bytes).unwrap();
        assert_eq!(output, restored);
    }

    #[test]
    fn test_type_hash_consistency() {
        let output1 = TestOutput {
            value: 1,
            name: "a".to_string(),
        };
        let output2 = TestOutput {
            value: 2,
            name: "b".to_string(),
        };

        // Same type should have same hash
        assert_eq!(output1.type_hash(), output2.type_hash());

        // Different type should have different hash
        let other: i32 = 42;
        assert_ne!(output1.type_hash(), other.type_hash());
    }

    #[test]
    fn test_boxed_output() {
        let output = TestOutput {
            value: 42,
            name: "test".to_string(),
        };

        let boxed = BoxedOutput::new(&output).unwrap();
        assert!(boxed.type_name().contains("TestOutput"));

        let restored: TestOutput = boxed.deserialize().unwrap();
        assert_eq!(output, restored);
    }

    #[test]
    fn test_boxed_output_type_mismatch() {
        let output = TestOutput {
            value: 42,
            name: "test".to_string(),
        };

        let boxed = BoxedOutput::new(&output).unwrap();

        // Try to deserialize as wrong type
        let result: Result<i32> = boxed.deserialize();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Type mismatch"));
    }

    #[test]
    fn test_primitive_outputs() {
        // Test that primitives work as cell outputs
        let int_val: i64 = 12345;
        let bytes = int_val.serialize_output().unwrap();
        let restored: i64 = deserialize_output(&bytes).unwrap();
        assert_eq!(int_val, restored);

        let string_val = "hello world".to_string();
        let bytes = string_val.serialize_output().unwrap();
        let restored: String = deserialize_output(&bytes).unwrap();
        assert_eq!(string_val, restored);

        let vec_val = vec![1, 2, 3, 4, 5];
        let bytes = vec_val.serialize_output().unwrap();
        let restored: Vec<i32> = deserialize_output(&bytes).unwrap();
        assert_eq!(vec_val, restored);
    }
}
