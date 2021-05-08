use std::fs;

use anyhow::Result;

/// Read temperatur from a One-Wire file to a decimal Celcius
/// For example 18.5
pub fn read_temperature(path: &str) -> Result<f64> {
    let contents = fs::read_to_string(path)?;
    let re = Regex::new(r"(?m)t=([0-9]+)$").unwrap();
    let caps = re.captures(&contents).unwrap();
    caps.get(1).unwrap().as_str().parse::<f64>().unwrap() / 1000.0
}
