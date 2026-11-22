use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
use std::sync::{Arc, atomic};
use std::thread;
use std::thread::{JoinHandle, sleep};
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;

use crate::common::progress_data::{ProgressData, ToolStage};
pub const LOOP_DURATION: u32 = 20;
pub const SEND_PROGRESS_DATA_TIME_BETWEEN: u32 = 200;

pub(crate) struct ProgressThreadHandler {
    // None when there is no progress receiver, so no thread to run.
    progress_thread_handle: Option<JoinHandle<()>>,
    progress_thread_running: Arc<AtomicBool>,
    items_counter: Arc<AtomicUsize>,
    size_counter: Arc<AtomicU64>,
}
impl ProgressThreadHandler {
    pub fn join_thread(self) {
        self.progress_thread_running.store(false, atomic::Ordering::Relaxed);
        if let Some(handle) = self.progress_thread_handle {
            handle
                .join()
                .expect("Cannot join progress thread - quite fatal error, but I hope, that it will never happen :)");
        }
    }
    pub fn increase_items(&self, count: usize) {
        self.items_counter.fetch_add(count, atomic::Ordering::Relaxed);
    }
    pub fn increase_size(&self, size: u64) {
        self.size_counter.fetch_add(size, atomic::Ordering::Relaxed);
    }
    pub fn items_counter(&self) -> &Arc<AtomicUsize> {
        &self.items_counter
    }
    pub fn size_counter(&self) -> &Arc<AtomicU64> {
        &self.size_counter
    }
}

pub(crate) fn prepare_thread_handler_common(progress_sender: Option<&Sender<ProgressData>>, stage: ToolStage, max_items: usize, max_size: u64) -> ProgressThreadHandler {
    let items_counter = Arc::new(AtomicUsize::new(0));
    let size_counter = Arc::new(AtomicU64::new(0));
    let progress_thread_running = Arc::new(AtomicBool::new(true));

    let progress_thread_handle = progress_sender.cloned().map(|progress_sender| {
        let items_counter = items_counter.clone();
        let size_counter = size_counter.clone();
        let progress_thread_running = progress_thread_running.clone();
        thread::spawn(move || {
            // Use earlier time, to send immediately first message
            let mut time_since_last_send = Instant::now().checked_sub(Duration::from_secs(10u64)).unwrap_or_else(Instant::now);

            loop {
                if time_since_last_send.elapsed().as_millis() > SEND_PROGRESS_DATA_TIME_BETWEEN as u128 {
                    let progress_data = ProgressData {
                        stage,
                        entries_checked: items_counter.load(atomic::Ordering::Relaxed),
                        entries_to_check: max_items,
                        bytes_checked: size_counter.load(atomic::Ordering::Relaxed),
                        bytes_to_check: max_size,
                    };

                    progress_sender.send(progress_data).expect("Cannot send progress data");
                    time_since_last_send = Instant::now();
                }
                if !progress_thread_running.load(atomic::Ordering::Relaxed) {
                    break;
                }
                sleep(Duration::from_millis(LOOP_DURATION as u64));
            }
        })
    });
    ProgressThreadHandler {
        progress_thread_handle,
        progress_thread_running,
        items_counter,
        size_counter,
    }
}

#[inline]
pub(crate) fn check_if_stop_received(stop_flag: &Arc<AtomicBool>) -> bool {
    stop_flag.load(atomic::Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::progress_data::DuplicateStage;

    #[test]
    fn test_stop_flag() {
        let stop_flag = Arc::new(AtomicBool::new(false));
        assert!(!check_if_stop_received(&stop_flag));
        stop_flag.store(true, atomic::Ordering::Relaxed);
        assert!(check_if_stop_received(&stop_flag));
    }

    #[test]
    fn test_progress_thread_handler_with_sender() {
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let handler = prepare_thread_handler_common(Some(&sender), ToolStage::Duplicate(DuplicateStage::FullHashing), 100, 10000);

        assert_eq!(handler.items_counter().load(atomic::Ordering::Relaxed), 0);
        assert_eq!(handler.size_counter().load(atomic::Ordering::Relaxed), 0);

        handler.increase_items(5);
        handler.increase_size(512);
        handler.increase_items(3);
        handler.increase_size(256);

        assert_eq!(handler.items_counter().load(atomic::Ordering::Relaxed), 8);
        assert_eq!(handler.size_counter().load(atomic::Ordering::Relaxed), 768);

        handler.join_thread();
    }

    #[test]
    fn test_progress_thread_handler_without_sender() {
        let handler = prepare_thread_handler_common(None, ToolStage::CollectingFiles(crate::common::model::CheckingMethod::None), 50, 5000);

        handler.increase_items(10);
        handler.increase_size(1000);

        assert_eq!(handler.items_counter().load(atomic::Ordering::Relaxed), 10);
        assert_eq!(handler.size_counter().load(atomic::Ordering::Relaxed), 1000);

        handler.join_thread();
    }
}
