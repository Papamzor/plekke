use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crossbeam_channel::Sender;

use crate::common::deletion;
pub use crate::common::deletion::{DeleteItemType, DeleteResult};
use crate::common::directories::Directories;
use crate::common::extensions::Extensions;
use crate::common::items::ExcludedItems;
use crate::common::model::{CheckingMethod, ToolType};
use crate::common::progress_data::ProgressData;
use crate::common::traits::ResultEntry;
use crate::helpers::messages::Messages;

#[derive(Debug, Clone, Default)]
pub struct CommonToolData {
    pub(crate) tool_type: ToolType,
    pub(crate) text_messages: Messages,
    pub(crate) directories: Directories,
    pub(crate) extensions: Extensions,
    pub(crate) excluded_items: ExcludedItems,
    pub(crate) recursive_search: bool,
    pub(crate) delete_method: DeleteMethod,
    pub(crate) maximal_file_size: u64,
    pub(crate) minimal_file_size: u64,
    pub(crate) stopped_search: bool,
    pub(crate) use_cache: bool,
    pub(crate) delete_outdated_cache: bool,
    pub(crate) use_reference_folders: bool,
    pub(crate) dry_run: bool,
    pub(crate) move_to_trash: bool,
    pub(crate) hide_hard_links: bool,
}

#[derive(Eq, PartialEq, Clone, Debug, Copy, Default)]
pub enum DeleteMethod {
    #[default]
    None,
    Delete, // Just delete items
    AllExceptNewest,
    AllExceptOldest,
    OneOldest,
    OneNewest,
    HardLink,
    AllExceptBiggest,
    AllExceptSmallest,
    OneBiggest,
    OneSmallest,
}

impl CommonToolData {
    pub fn new(tool_type: ToolType) -> Self {
        Self {
            tool_type,
            recursive_search: true,
            maximal_file_size: u64::MAX,
            use_cache: true,
            delete_outdated_cache: true,
            ..Default::default()
        }
    }
}

pub trait CommonData {
    type Info;
    type Parameters;

    fn get_information(&self) -> Self::Info;
    fn get_params(&self) -> &Self::Parameters;

    fn get_cd(&self) -> &CommonToolData;
    fn get_cd_mut(&mut self) -> &mut CommonToolData;
    fn get_check_method(&self) -> CheckingMethod {
        CheckingMethod::None
    }
    fn found_any_items(&self) -> bool;

    fn set_hide_hard_links(&mut self, hide_hard_links: bool) {
        self.get_cd_mut().hide_hard_links = hide_hard_links;
    }
    fn get_hide_hard_links(&self) -> bool {
        self.get_cd().hide_hard_links
    }

    fn set_dry_run(&mut self, dry_run: bool) {
        self.get_cd_mut().dry_run = dry_run;
    }

    fn set_use_cache(&mut self, use_cache: bool) {
        self.get_cd_mut().use_cache = use_cache;
    }
    fn get_use_cache(&self) -> bool {
        self.get_cd().use_cache
    }

    fn set_delete_outdated_cache(&mut self, delete_outdated_cache: bool) {
        self.get_cd_mut().delete_outdated_cache = delete_outdated_cache;
    }
    fn get_delete_outdated_cache(&self) -> bool {
        self.get_cd().delete_outdated_cache
    }

    fn get_stopped_search(&self) -> bool {
        self.get_cd().stopped_search
    }

    fn set_maximal_file_size(&mut self, maximal_file_size: u64) {
        self.get_cd_mut().maximal_file_size = maximal_file_size.max(1);
    }

    fn set_minimal_file_size(&mut self, minimal_file_size: u64) {
        self.get_cd_mut().minimal_file_size = minimal_file_size.max(1);
    }

    #[cfg(target_family = "unix")]
    fn set_exclude_other_filesystems(&mut self, exclude_other_filesystems: bool) {
        self.get_cd_mut().directories.set_exclude_other_filesystems(exclude_other_filesystems);
    }
    #[cfg(not(target_family = "unix"))]
    fn set_exclude_other_filesystems(&mut self, _exclude_other_filesystems: bool) {}

    fn get_text_messages(&self) -> &Messages {
        &self.get_cd().text_messages
    }
    fn get_text_messages_mut(&mut self) -> &mut Messages {
        &mut self.get_cd_mut().text_messages
    }

    fn set_recursive_search(&mut self, recursive_search: bool) {
        self.get_cd_mut().recursive_search = recursive_search;
    }

    fn set_use_reference_folders(&mut self, use_reference_folders: bool) {
        self.get_cd_mut().use_reference_folders = use_reference_folders;
    }
    fn get_use_reference_folders(&self) -> bool {
        self.get_cd().use_reference_folders
    }

    fn set_delete_method(&mut self, delete_method: DeleteMethod) {
        self.get_cd_mut().delete_method = delete_method;
    }

    // Only used for internal deleting - probably only useful in CLI, but not in GUI which probably uses its own delete method selection
    fn set_move_to_trash(&mut self, move_to_trash: bool) {
        self.get_cd_mut().move_to_trash = move_to_trash;
    }

    fn set_included_paths(&mut self, included_paths: Vec<PathBuf>) {
        let messages = self.get_cd_mut().directories.set_included_paths(included_paths);
        self.get_cd_mut().text_messages.extend_with_another_messages(messages);
    }

    fn set_excluded_paths(&mut self, excluded_paths: Vec<PathBuf>) {
        let messages = self.get_cd_mut().directories.set_excluded_paths(excluded_paths);
        self.get_cd_mut().text_messages.extend_with_another_messages(messages);
    }

    fn set_reference_paths(&mut self, reference_paths: Vec<PathBuf>) {
        let messages = self.get_cd_mut().directories.set_reference_paths(reference_paths);
        self.get_cd_mut().text_messages.extend_with_another_messages(messages);
    }

    fn set_allowed_extensions(&mut self, allowed_extensions: Vec<String>) {
        let messages = self.get_cd_mut().extensions.set_allowed_extensions(allowed_extensions);
        self.get_cd_mut().text_messages.extend_with_another_messages(messages);
    }

    fn set_excluded_extensions(&mut self, excluded_extensions: Vec<String>) {
        let messages = self.get_cd_mut().extensions.set_excluded_extensions(excluded_extensions);
        self.get_cd_mut().text_messages.extend_with_another_messages(messages);
    }

    fn set_excluded_items(&mut self, excluded_items: Vec<String>) {
        let messages = self.get_cd_mut().excluded_items.set_excluded_items(excluded_items);
        self.get_cd_mut().text_messages.extend_with_another_messages(messages);
    }

    fn get_extensions_mut(&mut self) -> &mut Extensions {
        &mut self.get_cd_mut().extensions
    }

    #[expect(clippy::result_unit_err)]
    fn prepare_items(&mut self, tool_extensions: Option<&[&str]>) -> Result<(), ()> {
        let recursive_search = self.get_cd().recursive_search;
        // Optimizes directories and removes recursive calls
        match self.get_cd_mut().directories.optimize_directories(recursive_search, false) {
            Ok(messages) => {
                self.get_cd_mut().text_messages.extend_with_another_messages(messages);
            }
            Err(messages) => {
                self.get_cd_mut().text_messages.extend_with_another_messages(messages);
                return Err(());
            }
        }

        if let Err(e) = self.get_extensions_mut().set_and_validate_extensions(tool_extensions) {
            self.get_cd_mut().text_messages.critical = Some(e);
            return Err(());
        }

        let cd = self.get_cd_mut();
        if cd.tool_type.may_use_reference_paths() {
            cd.use_reference_folders = !cd.directories.reference_directories.is_empty() || !cd.directories.reference_files.is_empty();
        }
        Ok(())
    }

    fn delete_simple_elements_and_add_to_messages<T: ResultEntry + Sized + Send + Sync>(
        &mut self,
        stop_flag: &Arc<AtomicBool>,
        progress_sender: Option<&Sender<ProgressData>>,
        delete_item_type: DeleteItemType<T>,
    ) -> ControlFlow<()> {
        deletion::delete_simple_elements_and_add_to_messages(self.get_cd_mut(), stop_flag, progress_sender, delete_item_type)
    }

    /// Deletes what `items` builds when the delete method is [`DeleteMethod::Delete`], the only one tools without groups support.
    fn delete_if_requested<T: ResultEntry + Sized + Send + Sync>(
        &mut self,
        stop_flag: &Arc<AtomicBool>,
        progress_sender: Option<&Sender<ProgressData>>,
        items: impl FnOnce(&Self) -> DeleteItemType<T>,
    ) -> ControlFlow<()> {
        // Simple-list tools have no "keep one per group" notion, so any method other than None means delete
        if self.get_cd().delete_method == DeleteMethod::None {
            return ControlFlow::Continue(());
        }
        let items = items(self);
        self.delete_simple_elements_and_add_to_messages(stop_flag, progress_sender, items)
    }

    fn delete_advanced_elements_and_add_to_messages<T: ResultEntry + Sized + Send + Sync + Clone>(
        &mut self,
        stop_flag: &Arc<AtomicBool>,
        progress_sender: Option<&Sender<ProgressData>>,
        files_to_process: Vec<Vec<T>>,
    ) -> ControlFlow<()> {
        deletion::delete_advanced_elements_and_add_to_messages(self.get_cd_mut(), stop_flag, progress_sender, files_to_process)
    }

    fn delete_elements<T: ResultEntry + Sized + Send + Sync>(
        &self,
        stop_flag: &Arc<AtomicBool>,
        progress_sender: Option<&Sender<ProgressData>>,
        delete_item_type: DeleteItemType<T>,
    ) -> DeleteResult {
        deletion::delete_elements(self.get_cd(), stop_flag, progress_sender, delete_item_type)
    }
}

/// Implements [`CommonData`] for a tool with `common_data`, `information` and (unless the parameter type is `()`)
/// `params` fields. Tool-specific methods such as `found_any_items` follow the three types.
macro_rules! impl_common_data {
    (@impl $tool:ty, $info:ty, $params:ty, $($methods:tt)*) => {
        impl $crate::common::tool_data::CommonData for $tool {
            type Info = $info;
            type Parameters = $params;

            fn get_information(&self) -> Self::Info {
                self.information
            }
            fn get_cd(&self) -> &$crate::common::tool_data::CommonToolData {
                &self.common_data
            }
            fn get_cd_mut(&mut self) -> &mut $crate::common::tool_data::CommonToolData {
                &mut self.common_data
            }
            $($methods)*
        }
    };
    ($tool:ty, $info:ty, (), $($methods:tt)*) => {
        $crate::common::tool_data::impl_common_data!(@impl $tool, $info, (), fn get_params(&self) -> &Self::Parameters { &() } $($methods)*);
    };
    ($tool:ty, $info:ty, $params:ty, $($methods:tt)*) => {
        $crate::common::tool_data::impl_common_data!(@impl $tool, $info, $params, fn get_params(&self) -> &Self::Parameters { &self.params } $($methods)*);
    };
}
pub(crate) use impl_common_data;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_common_tool_data_new() {
        let tool_data = CommonToolData::new(ToolType::Duplicate);
        assert_eq!(tool_data.tool_type, ToolType::Duplicate);
        assert_eq!(tool_data.delete_method, DeleteMethod::None);
        assert_eq!(tool_data.maximal_file_size, u64::MAX);
        assert_eq!(tool_data.minimal_file_size, 0);
        assert!(tool_data.recursive_search);
        assert!(!tool_data.stopped_search);
        assert!(tool_data.use_cache);
        assert!(tool_data.delete_outdated_cache);
        assert!(!tool_data.use_reference_folders);
        assert!(!tool_data.dry_run);
    }
}
