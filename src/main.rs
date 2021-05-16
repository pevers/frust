use actix::{Actor, Addr, Handler, Message, StreamHandler};
use actix_files::NamedFile;
use actix_web::{get, post, web, App, Error, HttpRequest, HttpResponse, HttpServer};
use actix_web_actors::ws;
use anyhow::{Context, Result};
use logs::FridgeStatusLog;
use core::f64;
use gpio::{Direction, Pin};
use log::info;
use pid::Pid;
use probes::read_temperature;
use serde::{Deserialize, Serialize};
use std::{
    env,
    path::Path,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use crate::logs::{read_logs, write_log};

mod gpio;
mod probes;
mod logs;

// Minimum of 90 s before turning the compressor on again
const MINIMUM_IDLE_TIME_MS: f64 = 90000.0;

// Minimum of 15 s of cooling before turning it off
const MINIMUM_COOL_TIME_MS: f64 = 15000.0;

// Duty cycle time in ms
const DUTY_CYCLE_MS: f64 = 300000.0;

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
struct Config {
    pub target_temp: f64,
    pub p: f64,
    pub i: f64,
    pub d: f64,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
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
    let mut pid = data.pid.lock().unwrap();
    pid.kp = config_update.p;
    pid.ki = config_update.i;
    pid.kd = config_update.d;
    pid.reset_integral_term();
    info!("Configuration updated {:?}", config_update);
    Ok(HttpResponse::Ok().json(*config_update))
}

// Upgrades connection to a Websocket and registers a listener
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

// Get the log for a certain date and hour
#[get("/api/logs/{date}/{hour}")]
async fn get_log(web::Path((date, hour)): web::Path<(String, String)>) -> actix_web::Result<HttpResponse> {
    let logs = read_logs(&date, &hour).expect("could not read logs");
    Ok(HttpResponse::Ok().json(logs))
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    // Temperature probes
    let outside_sensor_path = env::var("OUTSIDE_SENSOR").expect("OUTSIDE_SENSOR path not set");
    let inside_sensor_path = env::var("INSIDE_SENSOR").expect("INSIDE_SENSOR path not set");

    // Set compressor and heater GPIO pins
    let compressor = Pin::new(23)
        .export()
        .context("could not export compressor pin")?
        .set_direction(Direction::Out)?
        .set_value(0)?;

    // TODO: Find out the heater GPIO pin

    // Configuration, will be read from file and stored in a file
    // TODO: Read from file
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
    let config = Arc::new(Mutex::new(config));
    let pid = Arc::new(Mutex::new(pid));

    // Current status, will be updated by the control loop
    let mut status = FridgeStatus {
        inside_temp: 10.0,
        outside_temp: 10.0,
        correction: 0.0,
        mode: Mode::Idle,
        duty_cycle: 0.0,
        target_duty_cycle: 0.0,
    };

    // All listeners for fridge status updates
    let listeners: Arc<Mutex<Vec<Addr<FridgeStatusActor>>>> = Arc::new(Mutex::new(Vec::new()));

    let control_pid = pid.clone();
    let control_listeners = listeners.clone();
    let mut now = Instant::now();
    thread::spawn(move || {
        loop {
            let delta_ms: f64 = now.elapsed().as_millis() as f64;
            now = Instant::now();
            status.outside_temp =
                read_temperature(&outside_sensor_path).expect("Could not read outside temperature");
            status.inside_temp =
                read_temperature(&inside_sensor_path).expect("Could not read inside temperature");

            // TODO: Monitoring? Or just stick to the Bash script
            // Scoped block to quickly update configuration and release the lock
            {
                let mut control_pid = control_pid.lock().unwrap();
                let correction = control_pid.next_control_output(status.inside_temp);
                status.correction = correction.output;
            }
            status.target_duty_cycle = (status.correction / 100.0).abs();

            // Update duty cycle
            match status.mode {
                Mode::Idle => {
                    // Update duty cycle
                    status.duty_cycle = DUTY_CYCLE_MS.max(status.duty_cycle - delta_ms);

                    // The 3 options are
                    // Idle -> Idle
                    // Idle -> Cooling
                    // Idle -> Heating

                    if status.duty_cycle < status.target_duty_cycle {
                        // Status needs to change
                        // TODO: Check if we meed the deadlines
                        if status.correction < 0.0 {
                            status.mode = Mode::Cooling;
                        }
                        if status.correction > 0.0 {
                            status.mode = Mode::Heating;
                        }
                    }
                }
                Mode::Cooling => {
                    // Update duty cycle
                    status.duty_cycle = DUTY_CYCLE_MS.min(status.duty_cycle + delta_ms);

                    // The 2 options are
                    // Cooling -> Idle
                    // Cooling -> Cooling

                    // TODO: Check if we have been ON for long enough
                    if status.duty_cycle > status.target_duty_cycle {
                        status.mode = Mode::Idle;
                    }
                }
                Mode::Heating => {
                    status.duty_cycle = DUTY_CYCLE_MS.min(status.duty_cycle + delta_ms);

                    // The 2 options are
                    // Heating -> Idle
                    // Heating -> Heating

                    // TODO: Check if we have been ON for long enough
                    if status.duty_cycle > status.target_duty_cycle {
                        status.mode = Mode::Idle;
                    }
                }
            };

            info!("🍺 {:?} 🍺", status);
            write_log(status).expect("could not write log");

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
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await?;

    Ok(())
}
