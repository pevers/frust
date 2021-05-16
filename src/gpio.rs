//! Access GPIO pins via the old deprecated sysfs GPIO interface.
//! This is a simple synchronous read/write implementation
//! that does not depend on old Tokio/Mio versions.
//! It should therefore not be used for time sensitive hardware,
//! such as a rotary decoder.
//!
//! Subset taken from: https://github.com/rust-embedded/rust-sysfs-gpio/
use anyhow::{bail, Result};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pin {
    pin_num: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    In,
    Out,
    High,
    Low,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    NoInterrupt,
    RisingEdge,
    FallingEdge,
    BothEdges,
}

impl Pin {
    /// Create a new Pin with the provided `pin_num`
    ///
    /// This function does not export the provided pin_num.
    pub fn new(pin_num: u64) -> Pin {
        Pin { pin_num: pin_num }
    }

    /// Export the GPIO
    ///
    /// This is equivalent to `echo N > /sys/class/gpio/export` with
    /// the exception that the case where the GPIO is already exported
    /// is not an error.
    ///
    /// # Errors
    ///
    /// The main cases in which this function will fail and return an
    /// error are the following:
    /// 1. The system does not support the GPIO sysfs interface
    /// 2. The requested GPIO is out of range and cannot be exported
    /// 3. The requested GPIO is in use by the kernel and cannot
    ///    be exported by use in userspace
    pub fn export(&self) -> Result<&Pin> {
        if fs::metadata(&format!("/sys/class/gpio/gpio{}", self.pin_num)).is_err() {
            let mut export_file = File::create("/sys/class/gpio/export")?;
            export_file.write_all(format!("{}", self.pin_num).as_bytes())?;
        }
        Ok(self)
    }

    /// Set this GPIO as either an input or an output
    ///
    /// The basic values allowed here are `Direction::In` and
    /// `Direction::Out` which set the Pin as either an input
    /// or output respectively.  In addition to those, two
    /// additional settings of `Direction::High` and
    /// `Direction::Low`.  These both set the Pin as an output
    /// but do so with an initial value of high or low respectively.
    /// This allows for glitch-free operation.
    ///
    /// Note that this entry may not exist if the kernel does
    /// not support changing the direction of a pin in userspace.  If
    /// this is the case, you will get an error.
    pub fn set_direction(&self, dir: Direction) -> Result<&Pin> {
        self.write_to_device_file(
            "direction",
            match dir {
                Direction::In => "in",
                Direction::Out => "out",
                Direction::High => "high",
                Direction::Low => "low",
            },
        )?;
        Ok(self)
    }

    /// Set the value of the Pin
    ///
    /// This will set the value of the pin either high or low.
    /// A 0 value will set the pin low and any other value will
    /// set the pin high (1 is typical).
    pub fn set_value(&self, value: u8) -> Result<&Pin> {
        self.write_to_device_file(
            "value",
            match value {
                0 => "0",
                _ => "1",
            },
        )?;

        Ok(self)
    }

    /// Get the value of the Pin (0 or 1)
    ///
    /// If successful, 1 will be returned if the pin is high
    /// and 0 will be returned if the pin is low (this may or may
    /// not match the signal level of the actual signal depending
    /// on the GPIO "active_low" entry).
    pub fn get_value(&self) -> Result<u8> {
        match self.read_from_device_file("value") {
            Ok(s) => match s.trim() {
                "1" => Ok(1),
                "0" => Ok(0),
                other => bail!("value file contents {}", other),
            },
            Err(e) => Err(::std::convert::From::from(e)),
        }
    }

    /// Write all of the provided contents to the specified devFile
    fn write_to_device_file(&self, dev_file_name: &str, value: &str) -> io::Result<()> {
        let gpio_path = format!("/sys/class/gpio/gpio{}/{}", self.pin_num, dev_file_name);
        let mut dev_file = File::create(&gpio_path)?;
        dev_file.write_all(value.as_bytes())?;
        Ok(())
    }

    fn read_from_device_file(&self, dev_file_name: &str) -> io::Result<String> {
        let gpio_path = format!("/sys/class/gpio/gpio{}/{}", self.pin_num, dev_file_name);
        let mut dev_file = File::open(&gpio_path)?;
        let mut s = String::new();
        dev_file.read_to_string(&mut s)?;
        Ok(s)
    }
}
