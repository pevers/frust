use std::fs;

use anyhow::{Context, Result};
use regex::Regex;

/// Read temperatur from a One-Wire file to a decimal Celcius
/// For example 18.5
pub fn read_temperature(path: &str) -> Result<f64> {
    let contents = fs::read_to_string(path)?;
    let re = Regex::new(r"(?m)t=([0-9]+)$")?;
    let caps = re
        .captures(&contents)
        .context("capture failure for temperature")?;
    Ok(caps
        .get(1)
        .context("could not read temperature")?
        .as_str()
        .parse::<f64>()?
        / 1000.0)
}
