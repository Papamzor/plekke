//! Results of the tools that support reference folders (similar images, similar videos, same music).
//! Without reference folders results are plain groups; with them each group becomes
//! (reference file, files similar to it) and only the non-reference files count as duplicates.

use std::fmt::Debug;
use std::io::{self, Write};
use std::mem;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crossbeam_channel::Sender;
use serde::Serialize;

use crate::common::deletion::{DeleteItemType, delete_advanced_elements_and_add_to_messages, delete_simple_elements_and_add_to_messages};
use crate::common::progress_data::ProgressData;
use crate::common::tool_data::{CommonToolData, DeleteMethod};
use crate::common::traits::{PrintResults, ResultEntry};

/// Moves `groups` into `referenced` when reference folders are used; returns (number of groups, number of duplicates).
pub(crate) fn split_reference_groups<T: ResultEntry>(cd: &CommonToolData, groups: &mut Vec<Vec<T>>, referenced: &mut Vec<(T, Vec<T>)>) -> (usize, usize) {
    if cd.use_reference_folders {
        *referenced = cd.directories.filter_reference_folders(mem::take(groups));
        (referenced.len(), referenced.iter().map(|(_, files)| files.len()).sum())
    } else {
        (groups.len(), groups.iter().map(|group| group.len() - 1).sum())
    }
}

/// Applies the delete method: with reference folders to every non-reference file, otherwise per group.
pub(crate) fn delete_reference_groups<T: ResultEntry + Clone + Send + Sync>(
    cd: &mut CommonToolData,
    stop_flag: &Arc<AtomicBool>,
    progress_sender: Option<&Sender<ProgressData>>,
    groups: &[Vec<T>],
    referenced: &[(T, Vec<T>)],
) -> ControlFlow<()> {
    if cd.delete_method == DeleteMethod::None {
        return ControlFlow::Continue(());
    }
    if cd.use_reference_folders {
        let files_to_delete = referenced.iter().flat_map(|(_, files)| files.iter().cloned()).collect();
        return delete_simple_elements_and_add_to_messages(cd, stop_flag, progress_sender, DeleteItemType::DeletingFiles(files_to_delete));
    }
    delete_advanced_elements_and_add_to_messages(cd, stop_flag, progress_sender, groups.to_vec())
}

/// Text around the groups written by [`write_reference_groups`].
pub(crate) struct GroupHeaders {
    /// Follows the number of groups at the top of plain results.
    pub plain_summary: &'static str,
    /// Follows the number of groups at the top of referenced results.
    pub referenced_summary: &'static str,
    /// Follows "Found {files}" above each plain group.
    pub plain_group: &'static str,
    /// Follows "Found {files}" above each referenced group.
    pub referenced_group: &'static str,
    pub not_found: &'static str,
}

/// Writes `groups`, or `referenced` (reference file first in each group), or `not_found` when there are none.
pub(crate) fn write_reference_groups<W: Write, T>(
    writer: &mut W,
    groups: &[Vec<T>],
    referenced: &[(T, Vec<T>)],
    headers: &GroupHeaders,
    mut write_entry: impl FnMut(&mut W, &T) -> io::Result<()>,
) -> io::Result<()> {
    if !groups.is_empty() {
        write!(writer, "{}{}", groups.len(), headers.plain_summary)?;
        for group in groups {
            writeln!(writer, "Found {}{}", group.len(), headers.plain_group)?;
            for entry in group {
                write_entry(writer, entry)?;
            }
            writeln!(writer)?;
        }
    } else if !referenced.is_empty() {
        write!(writer, "{}{}", referenced.len(), headers.referenced_summary)?;
        for (reference, group) in referenced {
            writeln!(writer, "Found {}{}\n", group.len(), headers.referenced_group)?;
            write_entry(writer, reference)?;
            for entry in group {
                write_entry(writer, entry)?;
            }
            writeln!(writer)?;
        }
    } else {
        write!(writer, "{}", headers.not_found)?;
    }
    Ok(())
}

/// Saves the referenced groups as JSON when reference folders are used, the plain ones otherwise.
pub(crate) fn save_reference_groups_as_json<P: PrintResults, T: Serialize + Debug>(
    tool: &P,
    file_name: &str,
    groups: &[Vec<T>],
    referenced: &[(T, Vec<T>)],
    pretty_print: bool,
) -> io::Result<()> {
    if tool.get_cd().use_reference_folders {
        tool.save_results_to_file_as_json_internal(file_name, &referenced, pretty_print)
    } else {
        tool.save_results_to_file_as_json_internal(file_name, &groups, pretty_print)
    }
}
