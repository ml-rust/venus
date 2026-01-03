//! IPC protocol messages for Venus worker processes.
//!
//! Uses length-prefixed rkyv messages over stdin/stdout.
//! Format: 4-byte length (u32 LE) + rkyv-encoded message.

use std::io::{Read, Write};

use rkyv::{Archive, Deserialize, Serialize};

use crate::error::{Error, Result};

/// Command sent from parent to worker process.
#[derive(Debug, Clone, Archive, Serialize, Deserialize)]
pub enum WorkerCommand {
    /// Load a compiled cell's dynamic library.
    LoadCell {
        /// Path to the dylib file.
        dylib_path: String,
        /// Number of dependencies for FFI dispatch.
        dep_count: usize,
        /// Entry point symbol name.
        entry_symbol: String,
        /// Cell name for error reporting.
        name: String,
    },

    /// Execute the loaded cell with given inputs.
    Execute {
        /// Serialized inputs (rkyv bytes for each dependency).
        inputs: Vec<Vec<u8>>,
        /// Widget values as JSON (widget_id -> value).
        /// Empty if no widgets.
        widget_values_json: Vec<u8>,
    },

    /// Shutdown the worker process gracefully.
    Shutdown,

    /// Ping to check if worker is alive.
    Ping,
}

/// Response sent from worker to parent process.
#[derive(Debug, Clone, Archive, Serialize, Deserialize)]
pub enum WorkerResponse {
    /// Cell loaded successfully.
    Loaded,

    /// Execution completed successfully with output.
    Output {
        /// Serialized output bytes (display_len + display + rkyv data).
        bytes: Vec<u8>,
        /// Widget definitions as JSON.
        /// Empty if no widgets were registered.
        widgets_json: Vec<u8>,
    },

    /// Execution failed with an error.
    Error {
        /// Error message.
        message: String,
    },

    /// Worker panicked during execution.
    Panic {
        /// Panic message if available.
        message: String,
    },

    /// Response to Ping command.
    Pong,

    /// Acknowledgement of shutdown request.
    ShuttingDown,
}

/// Write a message to a writer using length-prefixed rkyv encoding.
pub fn write_message<W: Write>(
    writer: &mut W,
    message: &impl for<'a> Serialize<
        rkyv::rancor::Strategy<
            rkyv::ser::Serializer<
                rkyv::util::AlignedVec,
                rkyv::ser::allocator::ArenaHandle<'a>,
                rkyv::ser::sharing::Share,
            >,
            rkyv::rancor::Error,
        >,
    >,
) -> Result<()> {
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(message)
        .map_err(|e| Error::Serialization(format!("Failed to encode IPC message: {}", e)))?;

    let len = bytes.len() as u32;
    writer
        .write_all(&len.to_le_bytes())
        .map_err(|e| Error::Ipc(format!("Failed to write IPC message length: {}", e)))?;
    writer
        .write_all(&bytes)
        .map_err(|e| Error::Ipc(format!("Failed to write IPC message body: {}", e)))?;
    writer
        .flush()
        .map_err(|e| Error::Ipc(format!("Failed to flush IPC stream: {}", e)))?;

    Ok(())
}

/// Read a message from a reader using length-prefixed rkyv encoding.
///
/// # Safety
///
/// Uses unchecked deserialization for performance. Only safe when reading from
/// trusted sources (our own worker processes or state files).
pub fn read_message<R: Read, T>(reader: &mut R) -> Result<T>
where
    T: Archive,
    T::Archived: Deserialize<T, rkyv::rancor::Strategy<rkyv::de::Pool, rkyv::rancor::Error>>,
{
    let mut len_bytes = [0u8; 4];
    reader
        .read_exact(&mut len_bytes)
        .map_err(|e| Error::Ipc(format!("Failed to read IPC message length: {}", e)))?;
    let len = u32::from_le_bytes(len_bytes) as usize;

    // Sanity check: reject absurdly large messages (100MB)
    if len > 100 * 1024 * 1024 {
        return Err(Error::Ipc(format!(
            "IPC message too large: {} bytes",
            len
        )));
    }

    let mut bytes = vec![0u8; len];
    reader
        .read_exact(&mut bytes)
        .map_err(|e| Error::Ipc(format!("Failed to read IPC message body: {}", e)))?;

    // SAFETY: We trust data from our own worker processes and state files.
    // Using unchecked deserialization avoids CheckBytes trait complexity.
    let message = unsafe { rkyv::from_bytes_unchecked::<T, rkyv::rancor::Error>(&bytes) }
        .map_err(|e| Error::Serialization(format!("Failed to decode IPC message: {}", e)))?;

    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_command_roundtrip() {
        let cmd = WorkerCommand::LoadCell {
            dylib_path: "/tmp/cell.so".to_string(),
            dep_count: 2,
            entry_symbol: "venus_entry_my_cell".to_string(),
            name: "my_cell".to_string(),
        };

        let mut buf = Vec::new();
        write_message(&mut buf, &cmd).unwrap();

        let mut cursor = Cursor::new(buf);
        let decoded: WorkerCommand = read_message(&mut cursor).unwrap();

        match decoded {
            WorkerCommand::LoadCell {
                dylib_path,
                dep_count,
                entry_symbol,
                name,
            } => {
                assert_eq!(dylib_path, "/tmp/cell.so");
                assert_eq!(dep_count, 2);
                assert_eq!(entry_symbol, "venus_entry_my_cell");
                assert_eq!(name, "my_cell");
            }
            _ => panic!("Wrong command type"),
        }
    }

    #[test]
    fn test_response_roundtrip() {
        let resp = WorkerResponse::Output {
            bytes: vec![1, 2, 3, 4, 5],
            widgets_json: vec![],
        };

        let mut buf = Vec::new();
        write_message(&mut buf, &resp).unwrap();

        let mut cursor = Cursor::new(buf);
        let decoded: WorkerResponse = read_message(&mut cursor).unwrap();

        match decoded {
            WorkerResponse::Output { bytes, widgets_json } => {
                assert_eq!(bytes, vec![1, 2, 3, 4, 5]);
                assert!(widgets_json.is_empty());
            }
            _ => panic!("Wrong response type"),
        }
    }

    #[test]
    fn test_execute_command_roundtrip() {
        let cmd = WorkerCommand::Execute {
            inputs: vec![vec![1, 2, 3], vec![4, 5, 6]],
            widget_values_json: vec![],
        };

        let mut buf = Vec::new();
        write_message(&mut buf, &cmd).unwrap();

        let mut cursor = Cursor::new(buf);
        let decoded: WorkerCommand = read_message(&mut cursor).unwrap();

        match decoded {
            WorkerCommand::Execute { inputs, widget_values_json } => {
                assert_eq!(inputs.len(), 2);
                assert_eq!(inputs[0], vec![1, 2, 3]);
                assert_eq!(inputs[1], vec![4, 5, 6]);
                assert!(widget_values_json.is_empty());
            }
            _ => panic!("Wrong command type"),
        }
    }

    #[test]
    fn test_empty_execute_command() {
        // This tests the case that's failing in process_isolation tests
        let cmd = WorkerCommand::Execute {
            inputs: vec![],
            widget_values_json: vec![],
        };

        let mut buf = Vec::new();
        write_message(&mut buf, &cmd).unwrap();
        eprintln!("Empty Execute command serializes to {} bytes", buf.len());

        let mut cursor = Cursor::new(buf);
        let decoded: WorkerCommand = read_message(&mut cursor).unwrap();

        match decoded {
            WorkerCommand::Execute { inputs, widget_values_json } => {
                assert!(inputs.is_empty());
                assert!(widget_values_json.is_empty());
            }
            _ => panic!("Wrong command type"),
        }
    }

    #[test]
    fn test_loaded_response_size() {
        let response = WorkerResponse::Loaded;

        let mut buf = Vec::new();
        write_message(&mut buf, &response).unwrap();
        eprintln!("Loaded response serializes to {} bytes total ({} payload)",
                  buf.len(), buf.len() - 4);

        let mut cursor = Cursor::new(buf);
        let decoded: WorkerResponse = read_message(&mut cursor).unwrap();

        matches!(decoded, WorkerResponse::Loaded);
    }
}
