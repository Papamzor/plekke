use std::fs;

use log::{debug, error};
use serde::{Deserialize, Serialize};

use crate::common::cache::{CACHE_CLEANING_INTERVAL_SECONDS, CLEANING_TIMESTAMPS_FILE};
use crate::common::config_cache_path::get_config_cache_path;

#[derive(Deserialize, Serialize, Debug)]
struct CleaningTimestamps {
    timestamps: Vec<SingleCleaningTimestamp>,
}
#[derive(Deserialize, Serialize, Debug)]
struct SingleCleaningTimestamp {
    cache_file_name: String,
    last_cleaned_timestamp: u64,
}

fn get_timestamps_file_path() -> Option<std::path::PathBuf> {
    get_config_cache_path().map(|config| config.cache_folder.join(CLEANING_TIMESTAMPS_FILE))
}

pub(crate) fn should_clean_cache(cache_file_name: &str) -> bool {
    let Some(timestamps_file) = get_timestamps_file_path() else {
        return true;
    };

    let Ok(content) = fs::read_to_string(&timestamps_file) else {
        return true;
    };

    let cleaning_timestamps = match serde_json::from_str::<CleaningTimestamps>(&content) {
        Ok(t) => t,
        Err(e) => {
            error!(
                "Failed to parse cleaning timestamps file \"{}\" while processing cache file \"{cache_file_name}\" - {e:?}",
                timestamps_file.to_string_lossy()
            );
            return true;
        }
    };

    let current_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();

    if let Some(timestamp) = cleaning_timestamps.timestamps.iter().find(|t| t.cache_file_name == cache_file_name) {
        let time_since_last_cleaning = current_time.saturating_sub(timestamp.last_cleaned_timestamp);
        if time_since_last_cleaning < *CACHE_CLEANING_INTERVAL_SECONDS {
            debug!(
                "Last cleaning for {} was {} seconds ago, which is less than the configured interval of {} seconds. Skipping cleaning.",
                cache_file_name, time_since_last_cleaning, *CACHE_CLEANING_INTERVAL_SECONDS
            );
            return false;
        }
        debug!(
            "Last cleaning for {} was {} seconds ago, which exceeds the configured interval of {} seconds. Proceeding with cleaning.",
            cache_file_name, time_since_last_cleaning, *CACHE_CLEANING_INTERVAL_SECONDS
        );
        return true;
    }

    debug!("No cleaning timestamp found for {cache_file_name}, cache cleaning should run");
    true
}

pub(crate) fn update_cleaning_timestamp(cache_file_name: &str) {
    let Some(timestamps_file) = get_timestamps_file_path() else {
        return;
    };

    let mut cleaning_timestamps = if let Ok(content) = fs::read_to_string(&timestamps_file) {
        serde_json::from_str::<CleaningTimestamps>(&content).unwrap_or_else(|e| {
            error!("Failed to parse cleaning timestamps file \"{}\" content - {e:?}", timestamps_file.to_string_lossy());
            CleaningTimestamps { timestamps: Vec::new() }
        })
    } else {
        CleaningTimestamps { timestamps: Vec::new() }
    };

    let current_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();

    if let Some(timestamp) = cleaning_timestamps.timestamps.iter_mut().find(|t| t.cache_file_name == cache_file_name) {
        timestamp.last_cleaned_timestamp = current_time;
    } else {
        cleaning_timestamps.timestamps.push(SingleCleaningTimestamp {
            cache_file_name: cache_file_name.to_string(),
            last_cleaned_timestamp: current_time,
        });
    }

    if let Ok(serialized) = serde_json::to_string_pretty(&cleaning_timestamps) {
        if let Err(e) = fs::write(&timestamps_file, serialized) {
            error!("Failed to write cleaning timestamps to file {}: {e}", timestamps_file.to_string_lossy());
        }
    } else {
        error!("Failed to serialize cleaning timestamps");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::cache::tests::setup_cache_path;

    #[test]
    fn test_timestamp_operations_and_should_clean() {
        setup_cache_path();
        let cache_name = format!("test_cache_{}", std::process::id());

        assert!(should_clean_cache(&cache_name));

        update_cleaning_timestamp(&cache_name);
        assert!(!should_clean_cache(&cache_name));

        let different_cache = format!("different_cache_{}", std::process::id());
        assert!(should_clean_cache(&different_cache));

        update_cleaning_timestamp(&different_cache);
        assert!(!should_clean_cache(&different_cache));
        assert!(!should_clean_cache(&cache_name));
    }
}
