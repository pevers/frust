use std::{fs::{self, File, OpenOptions}, io::{BufRead, BufReader, Write}};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use chrono::prelude::*;

use crate::FridgeStatus;


#[derive(Debug, Copy, Clone, Deserialize, Serialize)]
pub struct FridgeStatusLog {
    pub timestamp: f64,
    pub status: FridgeStatus
}

// Write the current status to storage
pub fn write_log(status: FridgeStatus) -> Result<()> {
    let timestamp = Utc::now().timestamp();
    let filename = filename()?;
    fs::create_dir_all("log")?;
    let mut file = OpenOptions::new().write(true).create(true).append(true).open(filename)?;
    file.write(format!("{},{}", timestamp, serde_json::to_string(&status)?).as_bytes())?;
    Ok(())
}

// Read logs for a given day and hour
pub fn read_logs(date: &str, hour: &str) -> Result<Vec<FridgeStatusLog>> {
    let file = File::open(format!("log/{}-{}", date, hour))?;
    let reader = BufReader::new(file);
    let mut logs: Vec<FridgeStatusLog> = Vec::new();
    for line in reader.lines() {
        let line = line?;
        let (timestamp, status) = line.split_once(",").context("invalid log format")?;
        logs.push(FridgeStatusLog {
            timestamp: timestamp.parse()?,
            status: serde_json::from_str(status)?
        });
    };
    Ok(logs)
}

// Generate filename for a day and an hour
fn filename() -> Result<String> {
    let date = Utc::now().format("%Y-%m-%d-%H");
    Ok(format!("log/{}", date))
}