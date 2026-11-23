//! Core domain models, IPC wire envelopes, worker framing, and OCR text stabilizer.

pub mod framing;
pub mod ipc_types;
pub mod models;
pub mod stabilizer;

pub use framing::*;
pub use ipc_types::*;
pub use models::*;
pub use stabilizer::*;
