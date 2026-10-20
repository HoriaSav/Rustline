//! The `.rlp` routing pack: a compact, versioned file holding the timetable
//! (milestone M2) and the walking network (M4).
//!
//! The byte layout is documented on [`write_pack`] (spec §7.5).
//!
//! A `.rlp.zst` file is the same bytes compressed with zstd; [`read_pack`]
//! accepts both.

mod data;
mod disk;
mod error;
mod format;

pub use data::{
    TimetableData, NO_COORD, SEG_LEN_TOLERANCE, SEG_ONEWAY, SEG_STEPS, STREET_MAX_LAT,
    STREET_MAX_PIECE_M,
};
pub use disk::DiskStreets;
pub use error::PackError;
pub use format::{
    read_pack, read_pack_with_limit, verify_pack_file, write_pack, CheckedFile, Pack, PackMeta,
    DEFAULT_MAX_BODY_BYTES, FORMAT_VERSION, MAGIC,
};
