use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{Local, NaiveDateTime};
use vrcx_0_core::game_log_parser::{
    clean_location, convert_log_time_to_iso8601, parse_log_line_header, parse_room_log_event,
    LogLocationSnapshot, RoomLogEvent,
};
use vrcx_0_core::vrchat_log_reader::parse_output_log_file_timestamp;

#[derive(Clone)]
struct LogFileCandidate {
    path: PathBuf,
    file_name: String,
    timestamp: Option<NaiveDateTime>,
    modified: SystemTime,
}

pub fn scan_current_location_snapshot(log_dir: &Path) -> Option<LogLocationSnapshot> {
    let candidate = latest_output_log_candidate(log_dir)?;
    scan_log_file_location_snapshot(&candidate.path, &candidate.file_name)
}

fn latest_output_log_candidate(log_dir: &Path) -> Option<LogFileCandidate> {
    if !log_dir.exists() {
        return None;
    }

    let candidates: Vec<_> = fs::read_dir(log_dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() {
                return None;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("output_log_") || !name.ends_with(".txt") {
                return None;
            }
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            Some(LogFileCandidate {
                path,
                timestamp: parse_output_log_file_timestamp(&name),
                file_name: name,
                modified,
            })
        })
        .collect();

    candidates
        .iter()
        .filter(|candidate| candidate.timestamp.is_some())
        .max_by_key(|candidate| candidate.timestamp)
        .cloned()
        .or_else(|| {
            candidates
                .into_iter()
                .max_by_key(|candidate| candidate.modified)
        })
}

fn scan_log_file_location_snapshot(path: &Path, file_name: &str) -> Option<LogLocationSnapshot> {
    let file = File::open(path).ok()?;
    let reader = BufReader::with_capacity(65536, file);
    let mut recent_world_name = String::new();
    let mut current_location: Option<LogLocationSnapshot> = None;

    for line in reader.lines().map_while(Result::ok) {
        let trimmed = line.trim_end();
        let Some((line_date, content)) = parse_log_line_header(trimmed) else {
            continue;
        };
        let now_local = Local::now().naive_local();
        if line_date > now_local + chrono::Duration::minutes(61) {
            continue;
        }

        match parse_room_log_event(trimmed, content) {
            Some(RoomLogEvent::Entering { world_name }) => {
                recent_world_name = world_name.to_string();
            }
            Some(RoomLogEvent::Joining { location }) => {
                let location = clean_location(location);
                if !location.is_empty() {
                    current_location = Some(LogLocationSnapshot {
                        location,
                        world_name: recent_world_name.clone(),
                        created_at: convert_log_time_to_iso8601(trimmed),
                        file_name: file_name.to_string(),
                    });
                }
            }
            Some(RoomLogEvent::Left) => current_location = None,
            None => {}
        }
    }

    current_location
}

#[cfg(test)]
mod tests;
