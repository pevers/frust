extern crate mio;
extern crate mio_extras;
extern crate pid_control;
extern crate sysfs_gpio;

mod context;
mod lcd;
mod rotary;
mod config_listener;

use context::{Configuration, Context, Status};
use config_listener::ConfigListener;
use lcd::PrintStatus;
use pid_control::Controller;
use pid_control::PIDController;
use regex::Regex;
use rotary::Rotary;
use sysfs_gpio::{Direction, Edge};
use mio::{Events, Poll, PollOpt, Ready, Token};

use std::env;
use std::error::Error;
use std::fs;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const CLOCK: Token = Token(0);
const DATA: Token = Token(1);
const CONFIG_CHANGE: Token = Token(2);

const MINIMUM_IDLE_TIME_MS: f64 = 90000.0;
const MINIMUM_COOL_TIME_MS: f64 = 2000.0;
const CONFIGURATION_PATH: &str = "/home/pi/fridge.json";

fn main() -> Result<(), Box<dyn Error>> {
    let outside_sensor_path = match env::var("OUTSIDE_SENSOR") {
        Ok(path) => path,
        Err(_) => {
            panic!("OUTSIDE_SENSOR path not set");
        }
    };
    let inside_sensor_path = match env::var("INSIDE_SENSOR") {
        Ok(path) => path,
        Err(_) => {
            panic!("INSIDE_SENSOR path not set");
        }
    };

    // Read configuration
    let config_listener = ConfigListener::new(CONFIGURATION_PATH);
    let config = Configuration::load_from_path(CONFIGURATION_PATH).expect("Could not read configuration file");

    // Create mutext to share between threads
    let context = Arc::new(Mutex::new(Context {
        inside_temp: 0.0,
        outside_temp: 0.0,
        config: config,
    }));

    // Compressor
    let compressor = sysfs_gpio::Pin::new(23);
    compressor.set_direction(Direction::Out)?;
    compressor.set_value(0)?;   // Always disable compressor on start

    // LCD setup
    let lcd = Arc::new(Mutex::new(lcd::init_lcd()));

    // Clock pin
    let clock = sysfs_gpio::Pin::new(27);
    clock.set_direction(Direction::In)?;
    clock.set_edge(Edge::BothEdges)?;
    let clock_events = clock
        .get_async_poller()
        .unwrap();

    // Data pin
    let data = sysfs_gpio::Pin::new(22);
    data.set_direction(Direction::In)?;
    data.set_edge(Edge::BothEdges)?;
    let data_events = data
        .get_async_poller()
        .unwrap();

    // Setup event registry
    let poll = Poll::new()?;
    let mut events = Events::with_capacity(128);
    let mut rotary = Rotary::new();

    poll.register(&clock_events, CLOCK, Ready::readable(), PollOpt::edge())?;
    poll.register(&data_events, DATA, Ready::readable(), PollOpt::edge())?;
    poll.register(&config_listener, CONFIG_CHANGE, Ready::readable(), PollOpt::edge())?;

    // Ditch first OS event
    poll.poll(&mut events, None).unwrap();

    // Start listening to hardware interaction
    {
        let context = context.clone();
        let lcd = lcd.clone();
        thread::spawn(move || {
            loop {
                // Wait for events
                poll.poll(&mut events, None).unwrap();
                let mut context = context.lock().unwrap();
                let mut lcd = lcd.lock().unwrap();
                for event in &events {
                    match event.token() {
                        CLOCK => {
                            let clock_value = clock.get_value().unwrap();
                            let data_value = data.get_value().unwrap();
                            let dir = rotary.update(clock_value, data_value);
                            match dir {
                                rotary::Direction::Clockwise | rotary::Direction::CounterClockwise => {
                                    println!("Turning knob {:?}", dir);
                                    context.config.target_temp += if dir == rotary::Direction::Clockwise { 0.1 } else { -0.1 };
                                    context.config.write_to_path(CONFIGURATION_PATH).expect("Could not write to configuration file");
                                    lcd.update(*context);
                                }
                                _ => {}
                            }
                        },
                        CONFIG_CHANGE => {
                            println!("Configuration change, reloading!");
                            let config = Configuration::load_from_path(CONFIGURATION_PATH).expect("Unable to read configuration");
                            context.config = config;
                        }
                        _ => {}
                    }
                }
            }
        });
    }

    let context = context.clone();
    let lcd = lcd.clone();
    let mut controller = PIDController::new(config.p, config.i, config.d);
    controller.set_limits(-100.0, 100.0);
    let mut now = Instant::now();
    let mut status_ts = Instant::now();
    loop {
        // Get temperature and store in the context
        let outside_temp = read_temperature(&outside_sensor_path);
        let inside_temp = read_temperature(&inside_sensor_path);
        let mut context = context.lock().unwrap();
        context.outside_temp = outside_temp;
        context.inside_temp = inside_temp;

        // Set target and current temperature
        controller.set_target(context.config.target_temp);
        controller.p_gain = context.config.p;
        controller.i_gain = context.config.i;
        controller.d_gain = context.config.d;

        let delta = now.elapsed();
        now = Instant::now();
        let correction = controller.update(inside_temp, delta.as_secs() as f64);
        let compressor_state = compressor.get_value()?;
        let time_elapsed = status_ts.elapsed().as_millis() as f64;
        if correction > 0.0 {
            // Shutdown compressor if it has been on for the minimum amount of time
            if compressor_state != 0 && time_elapsed > MINIMUM_COOL_TIME_MS {
                println!("Disabling compressor");
                compressor.set_value(0)?;
                status_ts = Instant::now();
                context.config.status = Status::Idle;
                context.config.write_to_path(CONFIGURATION_PATH).expect("Could not write configuration file");
            }
        } else {
            // Enable compressor if it has been idle for the minimum amount of time
            if compressor_state != 1 && time_elapsed > MINIMUM_IDLE_TIME_MS {
                println!("Enabling compressor");
                compressor.set_value(1)?;
                status_ts = Instant::now();
                context.config.status = Status::Cooling;
                context.config.write_to_path(CONFIGURATION_PATH).expect("Could not write configuration file");
            }
        }

        println!("Current temperature {}", inside_temp);
        println!("Target {}", context.config.target_temp);
        println!("Correction {}", correction);

        // Print it to the LCD
        let mut lcd = lcd.lock().unwrap();
        lcd.update(*context);

        thread::sleep(Duration::from_millis(200));
    }
}

fn read_temperature(path: &str) -> f64 {
    let contents =
        fs::read_to_string(path).expect(&format!("Cannot read temperature probe {}", path));
    let re = Regex::new(r"(?m)t=([0-9]+)$").unwrap();
    let caps = re.captures(&contents).unwrap();

    caps.get(1).unwrap().as_str().parse::<f64>().unwrap() / 1000.0
}
