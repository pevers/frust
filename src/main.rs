use actix::{Actor, Addr, Handler, Message, StreamHandler};
use actix_files::NamedFile;
use actix_web::{get, post, web, App, Error, HttpRequest, HttpResponse, HttpServer, Result};
use actix_web_actors::ws;
use log::info;
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

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
    listeners: Arc<Mutex<Vec<Addr<FridgeStatusActor>>>>,
}

// Display the UI
#[get("/")]
async fn index() -> Result<NamedFile> {
    Ok(NamedFile::open(Path::new("static/index.html"))?)
}

// Update the configuration of the controller
#[post("/api/config")]
async fn update_config(data: web::Data<AppState>) -> Result<HttpResponse> {
    // TODO: update configuration and return it
    let mut config = data.config.lock().unwrap();
    config.p = 5.0; // TEST, should be payload
    info!("Configuration {:?}", config);
    Ok(HttpResponse::Ok().json(*config))
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
    info!("Actor connected");
    let mut listeners = data.listeners.lock().unwrap();
    listeners.push(actor);
    Ok(resp)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    // Configuration, will be read from file and stored in a file
    let config = Arc::new(Mutex::new(Config {
        target_temp: 4.0,
        p: 1.0,
        i: 0.0,
        d: 0.0,
    }));

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
            // Scoped block to quickly update configuration and release the lock

            // TODO: Read control config and apply PID controller results
            // TODO: Read temperature probes
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
