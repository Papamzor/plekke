pub mod core;
#[cfg(test)]
mod tests;
pub mod traits;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::common::tool_data::CommonToolData;
use crate::common::traits::impl_result_entry;

#[derive(Debug, Default, Clone, Copy)]
pub struct Info {
    pub number_of_files_with_exif: usize,
    pub scanning_time: Duration,
}

#[derive(Clone, Default)]
pub struct ExifRemoverParameters {
    pub ignored_tags: Vec<String>,
}

impl ExifRemoverParameters {
    pub fn new(ignored_tags: Vec<String>) -> Self {
        Self { ignored_tags }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExifEntry {
    pub path: PathBuf,
    pub size: u64,
    pub modified_date: u64,
    pub exif_tags: Vec<ExifTagInfo>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExifTagInfo {
    pub name: String,
    pub code: u16,
    pub group: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExifTagsFixerParams {
    pub override_file: bool,
}

impl_result_entry!(ExifEntry);

pub struct ExifRemover {
    common_data: CommonToolData,
    information: Info,
    exif_files: Vec<ExifEntry>,
    files_to_check: BTreeMap<String, ExifEntry>,
    params: ExifRemoverParameters,
    additional_excluded_tags: BTreeMap<&'static str, Vec<&'static str>>,
}

impl ExifRemover {
    pub const fn get_exif_files(&self) -> &Vec<ExifEntry> {
        &self.exif_files
    }
}
