extern crate serde;
extern crate serde_json;

use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs::File;
use std::io::BufReader;

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum Status {
    Idle,
    Cooling,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct Configuration {
    pub target_temp: f64,
    pub p: f64,
    pub i: f64,
    pub d: f64,
}

impl Configuration {
    pub fn load_from_path(path: &str) -> Result<Configuration, Box<dyn Error>> {
        let f = File::open(path)?;
        let reader = BufReader::new(f);
        let config = serde_json::from_reader(reader)?;
        Ok(config)
    }

    pub fn write_to_path(self, path: &str) -> Result<(), Box<dyn Error>> {
        let f = File::create(path)?;
        serde_json::to_writer(&f, &self)?;
        Ok(())
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct Context {
    pub inside_temp: f64,
    pub outside_temp: f64,
    pub correction: f64,
    pub status: Status,
    pub config: Configuration,
}

impl Context {
    pub fn write_to_path(self, path: &str) -> Result<(), Box<dyn Error>> {
        let f = File::create(path)?;
        serde_json::to_writer(&f, &self)?;
        Ok(())
    }
}