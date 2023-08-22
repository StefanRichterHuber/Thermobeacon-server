use actix_web::{get, web, App, HttpResponse, HttpServer, Responder};
use serde_derive::Serialize;

use std::{error::Error, sync::Mutex};

#[derive(Serialize)]
pub struct Response {
    pub message: String,
}

pub enum HealthStatus {
    WaitingForFirstRun,
    LastRunFailed(String),
    Ok,
}

/// Global flag for current health status
static SYSTEM_STATUS: Mutex<HealthStatus> = Mutex::new(HealthStatus::WaitingForFirstRun);

#[get("/health")]
async fn healthcheck() -> impl Responder {
    let status = SYSTEM_STATUS.lock().unwrap();

    let result = match &*status {
        HealthStatus::WaitingForFirstRun => {
            debug!("Checked health of service: Waiting for the first run");
            let response = Response {
                message: "Waiting for the first run".to_string(),
            };
            HttpResponse::NotFound().json(response)
        }
        HealthStatus::LastRunFailed(msg) => {
            debug!("Checked health of service: Last run failed");
            let response = Response {
                message: msg.clone(),
            };
            HttpResponse::InternalServerError().json(response)
        }
        HealthStatus::Ok => {
            let response = Response {
                message: "Everything is working fine".to_string(),
            };
            HttpResponse::Ok().json(response)
        }
    };

    result
}

async fn not_found() -> actix_web::Result<HttpResponse> {
    let response = Response {
        message: "Resource not found".to_string(),
    };
    Ok(HttpResponse::NotFound().json(response))
}

/// Sets the current health status of thes
pub fn set_health_status(next_status: HealthStatus) {
    let mut status = SYSTEM_STATUS.lock().unwrap();
    *status = next_status;
}

/// Starts an actix web server for the health check endpoint
pub async fn start_healthcheck_server(ip: String, port: u16) -> Result<(), Box<dyn Error>> {
    let srv = HttpServer::new(|| {
        App::new()
            .service(healthcheck)
            .default_service(web::route().to(not_found))
    })
    .bind((ip, port))?
    .workers(1)
    .disable_signals()
    .run();

    tokio::spawn(srv);

    Ok(())
}
