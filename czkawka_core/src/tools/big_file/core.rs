use std::cmp::Reverse;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crossbeam_channel::Sender;
use fun_time::fun_time;
use log::debug;
use rayon::prelude::*;

use crate::common::model::ToolType;
use crate::common::progress_data::ProgressData;
use crate::common::tool_data::{CommonData, CommonToolData};
use crate::tools::big_file::{BigFile, BigFileParameters, Info, SearchMode};

impl BigFile {
    pub fn new(params: BigFileParameters) -> Self {
        Self {
            common_data: CommonToolData::new(ToolType::BigFile),
            information: Info::default(),
            big_files: Default::default(),
            params,
        }
    }

    #[fun_time(message = "look_for_big_files", level = "debug")]
    pub(crate) fn look_for_big_files(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) -> ControlFlow<()> {
        let mut all_files = self.common_data.collect_files(stop_flag, progress_sender, |traversal| {
            traversal.minimal_file_size = 1;
            traversal.maximal_file_size = u64::MAX;
        })?;

        if self.get_params().search_mode == SearchMode::BiggestFiles {
            all_files.par_sort_unstable_by_key(|fe| Reverse(fe.size));
        } else {
            all_files.par_sort_unstable_by_key(|fe| fe.size);
        }

        all_files.truncate(self.get_params().number_of_files_to_check);

        self.big_files = all_files;
        self.information.number_of_real_files = self.big_files.len();
        debug!("check_files - Found {} biggest/smallest files.", self.big_files.len());
        ControlFlow::Continue(())
    }
}
