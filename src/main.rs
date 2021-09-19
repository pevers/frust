use actix::{Actor, Addr, Handler, Message, StreamHandler};
use actix_files::NamedFile;
use actix_web::{error, get, post, web, App, Error, HttpRequest, HttpResponse, HttpServer};
use actix_web_actors::ws;
use anyhow::{Context, Result};
use core::f64;
use gpio::{Direction, Pin};
use lazy_static::lazy_static;
use log::info;
use pid::Pid;
use probes::read_temperature;
use prometheus::{opts, register_gauge, Encoder, Gauge, TextEncoder};
use serde::{Deserialize, Serialize};
use std::{
    env,
    path::Path,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

mod gpio;
mod probes;

// Minimum of 90 s before turning the compressor on again
const MINIMUM_IDLE_TIME_MS: f64 = 90000.0;

// Minimum of 15 s of cooling before turning it off
const MINIMUM_COOL_TIME_MS: f64 = 15000.0;

// Duty cycle time in ms
const DUTY_CYCLE_MS: f64 = 300000.0;

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
    static ref PID_CORRECTION: Gauge = register_gauge!(opts!(
        "pid_correction",
        "PID controller correction"
    )).unwrap();    
    static ref PID_P: Gauge = register_gauge!(opts!(
        "pid_p",
        "PID controller proportional gain"
    )).unwrap();
    static ref PID_I: Gauge = register_gauge!(opts!(
        "pid_i",
        "PID controller integral gain"
    )).unwrap();
    static ref PID_D: Gauge = register_gauge!(opts!(
        "pid_d",
        "PID controller derivative gain"
    )).unwrap();
    static ref COMPRESSOR: Gauge = register_gauge!(opts!(
        "compressor_activated",
        "Compressor is activated (1) or turned off (0)"
    )).unwrap();
}
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
struct Config {
    pub target_temp: f64,
    pub p: f64,
    pub i: f64,
    pub d: f64,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub enum Mode {
    Idle,
    Cooling,
    Heating,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct FridgeStatus {
    pub inside_temp: f64,
    pub outside_temp: f64,
    pub correction: f64,
    pub mode: Mode,             // Current mode, switching takes time
    pub mode_ms: f64,           // Amount of time spent in mode (ms)
    pub duty_cycle: f64,        // Current duty cycle (ms on / total duty cycle)
    pub target_duty_cycle: f64, // Target duty cycle (ms on)
}

#[derive(Message, Serialize)]
#[rtype(result = "()")]
struct FridgeStatusMessage {
    pub status: FridgeStatus,
}

/// Define HTTP actor
struct FridgeStatusActor;

impl Actor for FridgeStatusActor {
    type Context = ws::WebsocketContext<Self>;
}

impl Handler<FridgeStatusMessage> for FridgeStatusActor {
    type Result = ();

    fn handle(&mut self, msg: FridgeStatusMessage, ctx: &mut Self::Context) {
        ctx.text(serde_json::to_string(&msg).unwrap());
    }
}

/// Handler for ws::Message message
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for FridgeStatusActor {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Ping(msg)) => ctx.pong(&msg),
            _ => (),
        }
    }
}

struct AppState {
    config: Arc<Mutex<Config>>, // TODO: Can probably be removed
    pid: Arc<Mutex<Pid<f64>>>,
    listeners: Arc<Mutex<Vec<Addr<FridgeStatusActor>>>>,
}

// Display the UI
#[get("/")]
async fn index() -> actix_web::Result<NamedFile> {
    Ok(NamedFile::open(Path::new("static/index.html"))?)
}

// Update the configuration of the controller
#[post("/api/config")]
async fn update_config(
    data: web::Data<AppState>,
    config_update: web::Json<Config>,
) -> actix_web::Result<HttpResponse> {
    let mut temp = data.config.lock().unwrap();
    temp.target_temp = config_update.target_temp;
    let mut pid = data.pid.lock().unwrap();
    pid.kp = config_update.p;
    pid.ki = config_update.i;
    pid.kd = config_update.d;
    pid.reset_integral_term();
    info!("Configuration updated {:?}", config_update);
    Ok(HttpResponse::Ok().json(*config_update))
}

// Upgrades connection to a Websocket and registers a listener.
// Fridge status updates are send to all listeners
#[get("/api/ws")]
async fn status_update(
    req: HttpRequest,
    stream: web::Payload,
    data: web::Data<AppState>,
) -> actix_web::Result<HttpResponse, Error> {
    let (actor, resp) = ws::start_with_addr(FridgeStatusActor {}, &req, stream)?;
    let mut listeners = data.listeners.lock().unwrap();
    listeners.push(actor);
    info!("Actor connected");
    Ok(resp)
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
        _ => {
            COMPRESSOR.set(0.0);
        }
    }
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

    // Configuration, will be read from an ini file
    let config = Config {
        target_temp: 4.0,
        p: 1.0,
        i: 0.0,
        d: 0.0,
    };
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
    let mut status = FridgeStatus {
        inside_temp: 10.0,
        outside_temp: 10.0,
        correction: 0.0,
        mode: Mode::Idle,
        mode_ms: 0.0,
        duty_cycle: 0.0,
        target_duty_cycle: 0.0,
    };
    let config = Arc::new(Mutex::new(config));
    let pid = Arc::new(Mutex::new(pid));

    // All listeners for fridge status updates
    let listeners: Arc<Mutex<Vec<Addr<FridgeStatusActor>>>> = Arc::new(Mutex::new(Vec::new()));

    let control_pid = pid.clone();
    let control_listeners = listeners.clone();
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

            // TODO: Monitoring using Prometheus and AlertManager

            // Scoped block to quickly update configuration and release the lock
            {
                let mut control_pid = control_pid.lock().unwrap();
                let correction = control_pid.next_control_output(status.inside_temp);
                status.correction = correction.output;
            }
            status.target_duty_cycle = (status.correction / 100.0).abs() * DUTY_CYCLE_MS;

            // Update duty cycle
            match status.mode {
                Mode::Idle => {
                    // Update duty cycle
                    status.duty_cycle = MIN_DUTY_CYCLE_MS.max(status.duty_cycle - delta_ms);

                    // The 3 options are
                    // Idle -> Idle
                    // Idle -> Cooling
                    // Idle -> Heating

                    if status.duty_cycle < status.target_duty_cycle {
                        // Check if we need to turn the cooler/heater on
                        if status.correction < 0.0 && status.mode_ms >= MINIMUM_IDLE_TIME_MS {
                            enable_compressor(&compressor, &mut status)?;
                        } else if status.correction > 0.0 {
                            // Normally we would turn the heater on if enough
                            // time passed between the compressor idle time and the heater
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
                        // Do nothing
                    } else if status.duty_cycle > status.target_duty_cycle {
                        disable_compressor(&compressor, &mut status)?;
                    }
                }
                Mode::Heating => {
                    // TODO: For now we do nothing

                    status.duty_cycle = DUTY_CYCLE_MS.min(status.duty_cycle + delta_ms);

                    // The 2 options are
                    // Heating -> Idle
                    // Heating -> Heating
                }
            };

            info!("🍺 {:?} 🍺", status);
            status.mode_ms += delta_ms;

            // Write metrics for Prometheus
            {
                let mut config = control_config.lock().unwrap();
                write_metrics(&status, &config);
            }

            // Send status updates to all listeners
            for listener in control_listeners.lock().unwrap().iter() {
                listener.do_send(FridgeStatusMessage { status });
            }
            thread::sleep(Duration::from_millis(1000));
        }
    });

    HttpServer::new(move || {
        let state = web::Data::new(AppState {
            config: config.clone(),
            pid: pid.clone(),
            listeners: listeners.clone(),
        });
        App::new()
            .app_data(state.clone())
            .service(index)
            .service(update_config)
            .service(status_update)
            .service(get_metrics)
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await?;

    Ok(())
}
