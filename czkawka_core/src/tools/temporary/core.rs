use std::ffi::OsStr;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crossbeam_channel::Sender;
use fun_time::fun_time;

use crate::common::extensions::Extensions;
use crate::common::model::ToolType;
use crate::common::progress_data::ProgressData;
use crate::common::tool_data::CommonToolData;
use crate::tools::temporary::{Info, Temporary, TemporaryParameters};

impl Temporary {
    pub fn new(params: TemporaryParameters) -> Self {
        Self {
            common_data: CommonToolData::new(ToolType::TemporaryFiles),
            information: Info::default(),
            temporary_files: Vec::new(),
            params,
        }
    }

    #[fun_time(message = "check_files", level = "debug")]
    pub(crate) fn check_files(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) -> ControlFlow<()> {
        if self.params.extensions.is_empty() {
            self.common_data.text_messages.critical = Some("No temporary file extensions defined. Add at least one extension to search for.".to_string());
            return ControlFlow::Break(());
        }

        // ponytail: holds every file in memory before the name filter; add a filter hook to DirTraversal if huge trees need it.
        let files = self.common_data.collect_files(stop_flag, progress_sender, |traversal| {
            // This tool never applied directly included files or the extension/size filters.
            traversal.root_files.clear();
            traversal.extensions = Extensions::default();
            traversal.minimal_file_size = 0;
            traversal.maximal_file_size = u64::MAX;
        })?;

        let extensions = &self.params.extensions;
        self.temporary_files = files
            .into_iter()
            .filter(|fe| fe.path.file_name().is_some_and(|name| is_temporary_name(name, extensions)))
            .collect();
        self.information.number_of_temporary_files = self.temporary_files.len();

        ControlFlow::Continue(())
    }
}

fn is_temporary_name(file_name: &OsStr, extensions: &[String]) -> bool {
    let lowercase = file_name.to_ascii_lowercase();
    let lowercase = lowercase.to_string_lossy();
    extensions.iter().any(|ext| lowercase.ends_with(ext.as_str()))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use tempfile::TempDir;

    use crate::common::tool_data::CommonData;
    use crate::common::traits::Search;
    use crate::tools::temporary::{Temporary, TemporaryParameters};

    #[test]
    fn test_finds_temporary_files_recursively_by_lowercase_suffix() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path();
        fs::create_dir(path.join("sub")).unwrap();
        for name in ["x.tmp", "y.BAK", "~z", "keep.rs", "sub/b.temp", "sub/keep.txt"] {
            fs::write(path.join(name), b"a").unwrap();
        }

        let mut tool = Temporary::new(TemporaryParameters::default());
        tool.set_included_paths(vec![path.to_path_buf()]);
        tool.set_recursive_search(true);
        tool.search(&Arc::default(), None);

        let mut found: Vec<_> = tool
            .get_temporary_files()
            .iter()
            .map(|fe| fe.path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        found.sort();
        assert_eq!(found, ["b.temp", "x.tmp", "y.BAK"]);
    }
}
