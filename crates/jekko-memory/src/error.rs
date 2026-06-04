/// Errors raised by the memory service and its WAL persistence path.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    /// The injected WAL sink failed.
    #[error("wal sink: {0}")]
    Sink(String),
    /// A WAL op could not be serialized or deserialized for durable storage.
    #[error("wal op (de)serialize: {0}")]
    Serde(String),
    /// The persisted WAL records do not form a valid hash chain.
    #[error("wal chain break at seq {seq}: prev_hash {found} != expected {expected}")]
    ChainBreak {
        /// Sequence number of the offending record.
        seq: u64,
        /// `prev_hash` stored on the record.
        found: String,
        /// Hash of the preceding record, or empty for the first.
        expected: String,
    },
    /// Persisted WAL sequence numbers must be contiguous from 1.
    #[error("wal sequence break: found seq {found}, expected {expected}")]
    SequenceBreak {
        /// Sequence number found in durable storage.
        found: u64,
        /// Sequence number expected at this position.
        expected: u64,
    },
    /// A persisted WAL record does not match cogcore's recomputed hash.
    #[error("wal hash mismatch at seq {seq}: hash {found} != expected {expected}")]
    HashMismatch {
        /// Sequence number of the offending record.
        seq: u64,
        /// Hash stored on the record.
        found: String,
        /// Hash recomputed by cogcore replay.
        expected: String,
    },
}
