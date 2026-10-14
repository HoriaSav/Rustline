//! The `.rlp` routing pack: a compact, versioned file holding the timetable
//! (milestone M2; the street graph follows in M4).
//!
//! The byte layout is documented on [`write_pack`] (spec §7.5).
//!
//! A `.rlp.zst` file is the same bytes compressed with zstd; [`read_pack`]
//! accepts both.

mod data;
mod error;
mod format;

pub use data::{TimetableData, NO_COORD};
pub use error::PackError;
pub use format::{
    read_pack, read_pack_with_limit, write_pack, Pack, PackMeta, DEFAULT_MAX_BODY_BYTES,
    FORMAT_VERSION, MAGIC,
};
