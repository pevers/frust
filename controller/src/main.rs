extern crate mio;
extern crate mio_extras;
extern crate pid_control;
extern crate sysfs_gpio;

mod config_listener;
mod context;
mod lcd;
mod rotary;

use config_listener::ConfigListener;
use context::{Configuration, Context, Status};
use lcd::PrintStatus;
use mio::{Events, Poll, PollOpt, Ready, Token};
use pid_control::Controller;
use pid_control::PIDController;
use regex::Regex;
use rotary::Rotary;
use sysfs_gpio::{Direction, Edge};

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
const MINIMUM_COOL_TIME_MS: f64 = 15000.0;
const DUTY_CYCLE_S: f64 = MINIMUM_IDLE_TIME_MS * 2.0 / 1000.0;
const MIN_DUTY_CYCLE_S: f64 = 0.0;

// Ugly, but it works. Share settings via fridge.json and the current status/context via fridge-status.json
const CONFIGURATION_PATH: &str = "/etc/fridge.json";
const CONTEXT_PATH: &str = "/var/log/fridge-status.json";

// TODO: 1. Log data with Rust
// TODO: 2. Settings in dedicated file, only listen to this file for changes
// TODO: 3. Test export
// TODO: 4. non-root? move configuration file to application folder
// TODO: 5. Error log to dedicated file


fn main() -> Result<(), Box<dyn Error>> {
    let outside_sensor_path = env::var("OUTSIDE_SENSOR").expect("OUTSIDE_SENSOR path not set");
    let inside_sensor_path = env::var("INSIDE_SENSOR").expect("INSIDE_SENSOR path not set");

    // Read configuration
    let config_listener = ConfigListener::new(CONFIGURATION_PATH);
    let config = Configuration::load_from_path(CONFIGURATION_PATH)
        .expect("Could not read configuration file");

    // Create mutext to share between threads
    let context = Arc::new(Mutex::new(Context {
        inside_temp: 0.0,
        outside_temp: 0.0,
        correction: 0.0,
        status: Status::Idle,
        config: config,
    }));

    // Compressor
    let compressor = sysfs_gpio::Pin::new(23);
    compressor.export().expect("Could not export pin 23");
    compressor.set_direction(Direction::Out).expect("Could not set direction for GPIO pin 23");
    compressor.set_value(0).expect("Could not enable compressor");

    // LCD setup
    let lcd = Arc::new(Mutex::new(lcd::init_lcd()));

    // Clock pin
    let clock = sysfs_gpio::Pin::new(27);
    clock.export().expect("Could not export pin 27");
    clock.set_direction(Direction::In).expect("Could not set direction for GPIO pin 27");
    clock.set_edge(Edge::BothEdges).expect("Could not set edge for GPIO pin 27");
    let clock_events = clock.get_async_poller()?;

    // Data pin
    let data = sysfs_gpio::Pin::new(22);
    data.export().expect("Could not export pin 22");
    data.set_direction(Direction::In).expect("Could not set direction for GPIO pin 22");
    data.set_edge(Edge::BothEdges).expect("Could not set edge for GPIO pin 22");
    let data_events = data.get_async_poller()?;

    // Setup event registry
    let poll = Poll::new()?;
    let mut events = Events::with_capacity(128);
    let mut rotary = Rotary::new();

    poll.register(&clock_events, CLOCK, Ready::readable(), PollOpt::edge())?;
    poll.register(&data_events, DATA, Ready::readable(), PollOpt::edge())?;
    poll.register(
        &config_listener,
        CONFIG_CHANGE,
        Ready::readable(),
        PollOpt::edge(),
    )?;

    // Ditch first OS event
    poll.poll(&mut events, None)?;

    // Asynchronously start listening to events
    {
        let context = context.clone();
        let lcd = lcd.clone();
        thread::spawn(move || {
            loop {
                // Wait for events
                poll.poll(&mut events, None).unwrap();
                let mut context = context.unwrap();
                let mut lcd = lcd.lock().unwrap();
                for event in &events {
                    match event.token() {
                        CLOCK => {
                            let clock_value = clock.get_value().unwrap();
                            let data_value = data.get_value().unwrap();
                            let dir = rotary.update(clock_value, data_value);
                            match dir {
                                rotary::Direction::Clockwise
                                | rotary::Direction::CounterClockwise => {
                                    println!("Turning knob {:?}", dir);
                                    context.config.target_temp +=
                                        if dir == rotary::Direction::Clockwise {
                                            0.1
                                        } else {
                                            -0.1
                                        };
                                    context
                                        .config
                                        .write_to_path(CONFIGURATION_PATH)
                                        .expect("Could not write to configuration file");
                                    lcd.update(*context);
                                }
                                _ => {}
                            }
                        },
                        CONFIG_CHANGE => {
                            println!("Configuration change, reloading!");
                            let config = Configuration::load_from_path(CONFIGURATION_PATH)
                                .expect("Unable to read configuration");
                            context.config = config;
                        }
                        _ => {}
                    }
                }
            }
        });
    }

    let mut controller = PIDController::new(config.p, config.i, config.d);
    controller.set_limits(-100.0, 100.0);
    let mut now = Instant::now();
    let mut status_ts = Instant::now();
    let mut compressor_on: f64 = 0.0;
    let mut compressor_off: f64 = 0.0;
    loop {
        let context = context.clone();
        let lcd = lcd.clone();

        // Get temperature and store in the context
        let outside_temp = read_temperature(&outside_sensor_path)?;
        let inside_temp = read_temperature(&inside_sensor_path)?;
        let mut context = context.lock()?;
        context.outside_temp = outside_temp;
        context.inside_temp = inside_temp;

        // Set target and current temperature
        controller.set_target(context.config.target_temp);
        controller.p_gain = context.config.p;
        controller.i_gain = context.config.i;
        controller.d_gain = context.config.d;

        let delta = now.elapsed();
        let delta_sec = delta.as_secs_f64();
        now = Instant::now();
        let correction = controller.update(inside_temp, delta_sec);
        context.correction = correction;
        let compressor_state = compressor.get_value().expect("Could not get compressor state");
        let time_elapsed = status_ts.elapsed().as_millis() as f64;

        // Convert correction to PWM, 0 = no pulse, 100 = on for the entire cycle
        match compressor_state {
            0 => {
                compressor_off = DUTY_CYCLE_S.min(compressor_off + delta_sec);
                compressor_on = MIN_DUTY_CYCLE_S.max(compressor_on - delta_sec);
            }
            1 => {
                compressor_on = DUTY_CYCLE_S.min(compressor_on + delta_sec);
                compressor_off = MIN_DUTY_CYCLE_S.max(compressor_off - delta_sec);
            }
            _ => (),
        }

        let target_duty_cycle = correction / -100.0;
        let current_duty_cycle = compressor_on / DUTY_CYCLE_S;

        println!("🍺---------------------------🍺");
        println!("compressor_on {}", compressor_on);
        println!("compressor_off {}", compressor_off);
        println!("current_duty_cycle {}", current_duty_cycle);
        println!("target_duty_cycle {}", target_duty_cycle);
        println!("inside_temp {}", inside_temp);
        println!("target_temp {}", context.config.target_temp);
        println!("correction {}", correction);
        println!("🍺----------------------------🍺");

        if current_duty_cycle < target_duty_cycle {
            // Enable compressor if it has been idle for the minimum amount of time
            if compressor_state != 1 {
                if time_elapsed > MINIMUM_IDLE_TIME_MS {
                    println!("Enabling compressor");
                    compressor.set_value(1).expect("Could not enable compressor");
                    status_ts = Instant::now();
                    context.status = Status::Cooling;
                } else {
                    println!("Waiting for minimum charge period");
                }
            }
        } else {
            // Shutdown compressor if it has been on for the minimum amount of time
            if compressor_state != 0 {
                if time_elapsed > MINIMUM_COOL_TIME_MS {
                    println!("Disabling compressor");
                    compressor.set_value(0).expect("Could not disable compressor");
                    status_ts = Instant::now();
                    context.status = Status::Idle;
                } else {
                    println!("Waiting for minimum cool time");
                }
            }
        }
        context
            .write_to_path(CONTEXT_PATH)
            .expect("Could not write context file");

        // Print it to the LCD
        let mut lcd = lcd.lock()?;
        lcd.update(*context);

        thread::sleep(Duration::from_millis(1000));
    }
}

fn read_temperature(path: &str) -> Result<f64, Box<dyn Error>> {
    let contents = fs::read_to_string(path)?;
    let re = Regex::new(r"(?m)t=([0-9]+)$")?;
    let caps = re.captures(&contents).ok_or("Temperature reading not found")?;
    let cap = caps.get(1).ok_or("Cannot parse temperature")?;
    let temp: f64 = cap.as_str().parse()?;
    Ok(temp / 1000.0)
}
