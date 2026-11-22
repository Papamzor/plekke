pub mod core;
pub mod traits;

use std::time::Duration;

use crate::common::model::FileEntry;
use crate::common::tool_data::CommonToolData;

pub const TEMP_EXTENSIONS: &[&str] = &[
    "#",
    "thumbs.db",
    ".bak",
    "~",
    ".tmp",
    ".temp",
    ".ds_store",
    ".crdownload",
    ".part",
    ".cache",
    ".dmp",
    ".download",
    ".partial",
];

pub type TemporaryFileEntry = FileEntry;

#[derive(Default, Clone, Copy)]
pub struct Info {
    pub number_of_temporary_files: usize,
    pub scanning_time: Duration,
}

#[derive(Clone, Debug)]
pub struct TemporaryParameters {
    /// Full list of extensions/suffixes treated as temporary files.
    /// Each entry is matched against the lowercased filename using `ends_with`.
    /// Defaults to the built-in `TEMP_EXTENSIONS` list.
    /// Must not be empty - the core will return an error if it is.
    pub extensions: Vec<String>,
}

impl Default for TemporaryParameters {
    fn default() -> Self {
        Self {
            extensions: TEMP_EXTENSIONS.iter().map(|s| s.to_string()).collect(),
        }
    }
}

pub struct Temporary {
    common_data: CommonToolData,
    information: Info,
    temporary_files: Vec<TemporaryFileEntry>,
    params: TemporaryParameters,
}

impl Temporary {
    pub const fn get_temporary_files(&self) -> &Vec<TemporaryFileEntry> {
        &self.temporary_files
    }
}
