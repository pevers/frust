use actix::{Actor, Addr, Handler, Message, StreamHandler};
use actix_files::NamedFile;
use actix_web::{get, post, web, App, Error, HttpRequest, HttpResponse, HttpServer, Result};
use actix_web_actors::ws;
use log::info;
use pid::Pid;
use probes::read_temperature;
use serde::{Deserialize, Serialize};
use std::{
    env,
    path::Path,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
mod probes;

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
struct Config {
    pub target_temp: f64,
    pub p: f64,
    pub i: f64,
    pub d: f64,
}

#[derive(Debug, Copy, Clone, Serialize)]
struct FridgeStatus {
    pub inside_temp: f64,
    pub outside_temp: f64,
    pub correction: f64,
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
    config: Arc<Mutex<Config>>,
    pid: Arc<Mutex<Pid<f64>>>,
    listeners: Arc<Mutex<Vec<Addr<FridgeStatusActor>>>>,
}

// Display the UI
#[get("/")]
async fn index() -> Result<NamedFile> {
    Ok(NamedFile::open(Path::new("static/index.html"))?)
}

// Update the configuration of the controller
#[post("/api/config")]
async fn update_config(
    data: web::Data<AppState>,
    config_update: web::Json<Config>,
) -> Result<HttpResponse> {
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
) -> Result<HttpResponse, Error> {
    let (actor, resp) = ws::start_with_addr(FridgeStatusActor {}, &req, stream)?;
    let mut listeners = data.listeners.lock().unwrap();
    listeners.push(actor);
    info!("Actor connected");
    Ok(resp)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    // Temperature probes
    let outside_sensor_path = env::var("OUTSIDE_SENSOR").expect("OUTSIDE_SENSOR path not set");
    let inside_sensor_path = env::var("INSIDE_SENSOR").expect("INSIDE_SENSOR path not set");

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
        inside_temp: 0.0,
        outside_temp: 0.0,
        correction: 0.0,
    };

    // All listeners for fridge status updates
    let listeners: Arc<Mutex<Vec<Addr<FridgeStatusActor>>>> = Arc::new(Mutex::new(Vec::new()));

    let control_config = config.clone();
    let control_listeners = listeners.clone();
    thread::spawn(move || {
        loop {
            status.outside_temp =
                read_temperature(&outside_sensor_path).expect("Could not read outside temperature");
            status.inside_temp =
                read_temperature(&inside_sensor_path).expect("Could not read inside temperature");

            // TODO: Read temperature probes
            // TODO: Monitoring? Or just stick to the Bash script
            // Scoped block to quickly update configuration and release the lock
            {
                let mut control_config = control_config.lock().unwrap();

                control_config.p += 1.0;
                info!("Control loop {:?}", control_config);
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
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
