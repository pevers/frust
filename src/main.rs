use actix::Message;
use actix_files::NamedFile;
use actix_web::dev::ServiceRequest;
use actix_web::{error, get, web, App, Error, HttpResponse, HttpServer};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use actix_web_httpauth::middleware::HttpAuthentication;
use anyhow::{Context, Result};
use core::f64;
use gpio::Pin;
use lazy_static::lazy_static;
use log::info;
use pid::Pid;
use probes::read_temperature;
use prometheus::{opts, register_gauge, Encoder, Gauge, TextEncoder};
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::File,
    io::BufReader,
    path::Path,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use crate::gpio::Direction;

mod gpio;
mod probes;

// Wait an hour before switching between heating and cooling mode
const MINIMUM_HEATING_COOLING_SWITCH_TIME_MS: f64 = 3600000.0;

// Wait an hour before switching between cooling and heating mode
const MINIMUM_COOLING_HEATING_SWITCH_TIME_MS: f64 = 3600000.0;

// Minimum of 90 s before turning the compressor on again
const MINIMUM_IDLE_TIME_COOLING_MS: f64 = 90000.0;

// Wait 10 seconds before turning on the heater again
const MINIMUM_IDLE_TIME_HEATING_MS: f64 = 10000.0;

// Minimum of 15 s of cooling before turning it off
const MINIMUM_COOL_TIME_MS: f64 = 15000.0;

// Minimum of 30 seconds heating
const MINIMUM_HEAT_TIME_MS: f64 = 30000.0;

// Duty cycle time in ms
const DUTY_CYCLE_MS: f64 = 300000.0;

// Current duty cycle
const MIN_DUTY_CYCLE_MS: f64 = 0.0;

// All Prometheus metrics
lazy_static! {
    static ref INSIDE_TEMP_CELCIUS: Gauge = register_gauge!(opts!(
        "inside_temp_celcius",
        "Inside temperature of the fridge in Celcius"
    ))
    .unwrap();
    static ref OUTSIDE_TEMP_CELCIUS: Gauge = register_gauge!(opts!(
        "outside_temp_celcius",
        "Outside temperature of the room in Celcius"
    ))
    .unwrap();
    static ref TARGET_TEMP_CELCIUS: Gauge = register_gauge!(opts!(
        "target_temp_celcius",
        "Target temperature of the fridge in Celcius"
    ))
    .unwrap();
    static ref PID_CORRECTION: Gauge =
        register_gauge!(opts!("pid_correction", "PID controller correction")).unwrap();
    static ref PID_P: Gauge =
        register_gauge!(opts!("pid_p", "PID controller proportional gain")).unwrap();
    static ref PID_I: Gauge =
        register_gauge!(opts!("pid_i", "PID controller integral gain")).unwrap();
    static ref PID_D: Gauge =
        register_gauge!(opts!("pid_d", "PID controller derivative gain")).unwrap();
    static ref COMPRESSOR: Gauge = register_gauge!(opts!(
        "compressor_activated",
        "Compressor is activated (1) or turned off (0)"
    ))
    .unwrap();
    static ref HEATER: Gauge = register_gauge!(opts!(
        "heater_activated",
        "Heater is activated (1) or turned off (0)"
    ))
    .unwrap();
}
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
struct Config {
    // Mode of operation: Cooling or Heating
    pub operation_mode: OperationMode,
    pub target_temp: f64,
    pub p: f64,
    pub i: f64,
    pub d: f64,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            operation_mode: OperationMode::Heating,
            target_temp: 20.0,
            p: 8.0,
            i: 0.0,
            d: 0.0,
        }
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub enum Mode {
    Idle,
    Cooling,
    Heating,
}

// Mode of operation
// Either cooling or heating
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub enum OperationMode {
    Cooling,
    Heating,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct FridgeStatus {
    // Temperature in milli degrees
    pub inside_temp: f64,

    // Outside temp in milli degrees
    pub outside_temp: f64,

    // Correction from the PID controller
    pub correction: f64,

    // Current operation mode (heating or cooling)
    pub operation_mode: OperationMode,

    // Current mode while operational
    pub mode: Mode,

    // Amount of time spent in mode (ms)
    pub mode_ms: f64,

    // Current duty cycle (ms on / total duty cycle)
    pub duty_cycle: f64,

    // Target duty cycle (ms on)
    pub target_duty_cycle: f64,
}

impl Default for FridgeStatus {
    fn default() -> FridgeStatus {
        FridgeStatus {
            inside_temp: 10.0,
            outside_temp: 10.0,
            correction: 0.0,
            operation_mode: OperationMode::Heating,
            mode: Mode::Idle,
            mode_ms: 0.0,
            duty_cycle: 0.0,
            target_duty_cycle: 0.0,
        }
    }
}

#[derive(Message, Serialize)]
#[rtype(result = "()")]
struct FridgeStatusMessage {
    pub status: FridgeStatus,
}

struct AppState {
    config: Arc<Mutex<Config>>,
    pid: Arc<Mutex<Pid<f64>>>,
}

// Display the UI
#[get("/")]
async fn index() -> actix_web::Result<NamedFile> {
    Ok(NamedFile::open(Path::new("static/index.html"))?)
}

// Update the configuration of the controller
async fn update_config(
    data: web::Data<AppState>,
    config_update: web::Json<Config>,
) -> actix_web::Result<HttpResponse> {
    let update = Config {
        operation_mode: config_update.operation_mode,
        target_temp: config_update.target_temp,
        p: config_update.p,
        i: config_update.i,
        d: config_update.d,
    };
    let mut temp = data.config.lock().unwrap();
    temp.target_temp = update.target_temp;
    let mut pid = data.pid.lock().unwrap();
    pid.kp = update.p;
    pid.ki = update.i;
    pid.kd = update.d;
    pid.reset_integral_term();
    serde_json::to_writer(&File::create("config.json")?, &update)?;
    info!("Configuration updated {:?}", config_update);
    Ok(HttpResponse::Ok().json(*config_update))
}

#[get("/api/config")]
async fn get_config(data: web::Data<AppState>) -> actix_web::Result<HttpResponse> {
    let config = data.config.lock().unwrap().clone();
    Ok(HttpResponse::Ok().json(config))
}

#[get("/metrics")]
async fn get_metrics() -> actix_web::Result<HttpResponse> {
    let mut buffer = Vec::new();
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    encoder
        .encode(&metric_families, &mut buffer)
        .map_err(|e| error::ErrorInternalServerError(e))?;
    let output =
        String::from_utf8(buffer.clone()).map_err(|e| error::ErrorInternalServerError(e))?;
    Ok(HttpResponse::Ok().body(output))
}

// Protect update events with a bearer token
async fn validator(req: ServiceRequest, auth: BearerAuth) -> Result<ServiceRequest, Error> {
    let expected =
        env::var("TOKEN").map_err(|_| error::ErrorInternalServerError("Token not set"))?;
    if expected == auth.token() {
        return Ok(req);
    }
    Err(error::ErrorUnauthorized("Not authorized"))
}

// Enable the compressor and log the event and update the FridgeStatus
fn enable_compressor(compressor: &Pin, status: &mut FridgeStatus) -> Result<()> {
    info!("Enabling compressor!");
    compressor.set_value(1)?;
    status.mode = Mode::Cooling;
    status.mode_ms = 0.0;
    Ok(())
}

// Disable the compressor and log the event and update the FridgeStatus
fn disable_compressor(compressor: &Pin, status: &mut FridgeStatus) -> Result<()> {
    info!("Disabling compressor");
    compressor.set_value(0)?;
    status.mode = Mode::Idle;
    status.mode_ms = 0.0;
    Ok(())
}

// Enable the heater and log the event and update the FridgeStatus
fn enable_heater(heater: &Pin, status: &mut FridgeStatus) -> Result<()> {
    info!("Enabling heater!");
    heater.set_value(1)?;
    status.mode = Mode::Heating;
    status.mode_ms = 0.0;
    Ok(())
}

// Disable the heater and log the event and update the FridgeStatus
fn disable_heater(heater: &Pin, status: &mut FridgeStatus) -> Result<()> {
    info!("Disabling heater");
    heater.set_value(0)?;
    status.mode = Mode::Idle;
    status.mode_ms = 0.0;
    Ok(())
}

// Write metrics to the Prometheus collectors
fn write_metrics(status: &FridgeStatus, config: &Config) {
    INSIDE_TEMP_CELCIUS.set(status.inside_temp);
    OUTSIDE_TEMP_CELCIUS.set(status.outside_temp);
    TARGET_TEMP_CELCIUS.set(config.target_temp);
    PID_CORRECTION.set(status.correction);
    PID_P.set(config.p);
    PID_I.set(config.i);
    PID_D.set(config.d);
    match status.mode {
        Mode::Cooling => {
            COMPRESSOR.set(1.0);
        }
        Mode::Heating => {
            HEATER.set(1.0);
        }
        _ => {
            COMPRESSOR.set(0.0);
            HEATER.set(0.0);
        }
    }
}

fn read_config() -> Result<Config> {
    let file = File::open("config.json");
    if let Ok(f) = file {
        let config = serde_json::from_reader(BufReader::new(f))?;
        return Ok(config);
    }
    let config = Config::default();
    serde_json::to_writer(&File::create("config.json")?, &config)?;
    Ok(config)
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    // Temperature probes
    let outside_sensor_path = env::var("OUTSIDE_SENSOR").expect("OUTSIDE_SENSOR path not set");
    let inside_sensor_path = env::var("INSIDE_SENSOR").expect("INSIDE_SENSOR path not set");

    // Set compressor and heater GPIO pins
    let compressor = Pin::new(23);
    compressor
        .export()
        .context("could not export compressor pin")?
        .set_direction(Direction::Out)?
        .set_value(0)?;
    let heater = Pin::new(24);
    heater
        .export()
        .context("could not export heater pin")?
        .set_direction(Direction::Out)?
        .set_value(0)?;

    let config = read_config()?;
    let pid = Pid::new(
        config.p,
        config.i,
        config.d,
        100.0,
        100.0,
        100.0,
        100.0,
        config.target_temp,
    );
    // Current status, will be updated by the control loop
    let mut status = FridgeStatus::default();
    let config = Arc::new(Mutex::new(config));
    let pid = Arc::new(Mutex::new(pid));

    let control_pid = pid.clone();
    let control_config = config.clone();
    let mut now = Instant::now();
    thread::spawn(move || -> Result<()> {
        loop {
            let delta_ms: f64 = now.elapsed().as_millis() as f64;
            now = Instant::now();
            status.outside_temp =
                read_temperature(&outside_sensor_path).expect("Could not read outside temperature");
            status.inside_temp =
                read_temperature(&inside_sensor_path).expect("Could not read inside temperature");

            // Scoped block to quickly update configuration and release the lock
            {
                let mut control_pid = control_pid.lock().unwrap();
                let correction = control_pid.next_control_output(status.inside_temp);
                status.correction = correction.output;
            }
            status.target_duty_cycle = (status.correction / 100.0).abs() * DUTY_CYCLE_MS;

            // This is one big messy state machine, I'll create ASCII art soon
            // Basically, it works by having two operation modes cooling and heating.
            // You can only switch between the two if a long period has passed
            // to prevent any oscillation.
            match control_config.lock().unwrap().operation_mode {
                OperationMode::Cooling => {
                    // Update duty cycle
                    match status.mode {
                        Mode::Idle => {
                            // Update duty cycle
                            status.duty_cycle = MIN_DUTY_CYCLE_MS.max(status.duty_cycle - delta_ms);

                            // The 2 options are
                            // Idle -> Idle
                            // Idle -> Cooling

                            if status.correction < 0.0 {
                                // Check if we need to turn the cooler/heater on
                                if status.duty_cycle < status.target_duty_cycle
                                    && status.mode_ms >= MINIMUM_IDLE_TIME_COOLING_MS
                                {
                                    enable_compressor(&compressor, &mut status)?;
                                }
                                // We have cooled enough
                            } else {
                                // Possibly switch to heating
                                if status.mode_ms > MINIMUM_COOLING_HEATING_SWITCH_TIME_MS {
                                    info!("Switching to operation mode heating!");
                                    status.operation_mode = OperationMode::Heating;
                                    status.mode = Mode::Idle;
                                    status.mode_ms = 0.0;
                                }
                            }
                        }
                        Mode::Cooling => {
                            // Update duty cycle
                            status.duty_cycle = DUTY_CYCLE_MS.min(status.duty_cycle + delta_ms);

                            // The 2 options are
                            // Cooling -> Idle
                            // Cooling -> Cooling

                            if status.mode_ms < MINIMUM_COOL_TIME_MS {
                                // Do nothing because we keep cooling
                            } else if status.duty_cycle > status.target_duty_cycle {
                                disable_compressor(&compressor, &mut status)?;
                            }
                        }
                        _ => {
                            panic!("Invalid mode for operation Cooling");
                        }
                    }
                }
                OperationMode::Heating => {
                    // Update duty cycle
                    match status.mode {
                        Mode::Idle => {
                            // Update duty cycle
                            status.duty_cycle = MIN_DUTY_CYCLE_MS.max(status.duty_cycle - delta_ms);

                            // The 2 options are
                            // Idle -> Idle
                            // Idle -> Heating

                            if status.correction > 0.0 {
                                // Check if we need to turn the cooler/heater on
                                if status.duty_cycle < status.target_duty_cycle
                                    && status.mode_ms >= MINIMUM_IDLE_TIME_HEATING_MS
                                {
                                    enable_heater(&heater, &mut status)?;
                                }
                            } else {
                                // Possibly switch to cooling
                                if status.mode_ms > MINIMUM_HEATING_COOLING_SWITCH_TIME_MS {
                                    info!("Switching to operation mode cooling!");
                                    status.operation_mode = OperationMode::Cooling;
                                    status.mode = Mode::Idle;
                                    status.mode_ms = 0.0;
                                }
                            }
                        }
                        Mode::Heating => {
                            // Update duty cycle
                            status.duty_cycle = DUTY_CYCLE_MS.min(status.duty_cycle + delta_ms);

                            // The 2 options are
                            // Heating -> Idle
                            // Heating -> Heating

                            if status.mode_ms < MINIMUM_HEAT_TIME_MS {
                                // Do nothing
                            } else if status.duty_cycle > status.target_duty_cycle {
                                disable_heater(&heater, &mut status)?;
                            }
                        }
                        _ => {
                            panic!("Invalid mode for operation Heating");
                        }
                    }
                }
            }

            info!("🍺 {:?} 🍺", status);
            status.mode_ms += delta_ms;

            // Write metrics for Prometheus
            {
                let config = control_config.lock().unwrap();
                write_metrics(&status, &config);
            }

            thread::sleep(Duration::from_millis(1000));
        }
    });

    HttpServer::new(move || {
        let state = web::Data::new(AppState {
            config: config.clone(),
            pid: pid.clone(),
        });

        App::new()
            .app_data(state.clone())
            .service(index)
            .service(get_config)
            .service(
                web::resource("/api/config")
                    .route(web::post().to(update_config))
                    .wrap(HttpAuthentication::bearer(validator)),
            )
            .service(get_metrics)
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await?;

    Ok(())
}
