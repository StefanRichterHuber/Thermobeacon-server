extern crate paho_mqtt as mqtt;
extern crate pretty_env_logger;
#[macro_use]
extern crate log;

mod configuration;
mod health_check_server;
mod thermobeacon_protocol;

use btleplug::{api::BDAddr, platform::Manager};
use chrono::Utc;
use configuration::{AppDevice, MqttConfig};
use mqtt::AsyncClient;

use std::{error::Error, time::Duration};

use crate::{
    configuration::{read_configuration, AppConfig, DEFAULT_TIMEZONE},
    health_check_server::{start_healthcheck_server, HealthStatus, SYSTEM_STATUS},
};

/// Structure of MQTT message send
#[derive(Debug, Default, serde_derive::Serialize, PartialEq)]
struct Message {
    data: thermobeacon_protocol::ThermoBeaconFullReadResult,
    name: String,
}

// Tries to connect to the MQTT server using the given MqttConfig
async fn connect_to_mqtt(mqtt_config: &MqttConfig) -> Result<AsyncClient, Box<dyn Error>> {
    // Create the client
    let cli = mqtt::AsyncClient::new(mqtt_config.url.clone().unwrap()).unwrap();

    let conn_opts = if mqtt_config.password.is_some() && mqtt_config.username.is_some() {
        debug!(
            "Configuration of MQTT with user {} and password ***",
            mqtt_config.username.clone().unwrap()
        );

        mqtt::ConnectOptionsBuilder::new()
            .keep_alive_interval(Duration::from_secs(mqtt_config.keep_alive))
            .user_name(mqtt_config.username.clone().unwrap())
            .password(mqtt_config.password.clone().unwrap())
            .clean_session(true)
            .finalize()
    } else {
        debug!("Configuration of MQTT without username / password");
        mqtt::ConnectOptionsBuilder::new()
            .keep_alive_interval(Duration::from_secs(mqtt_config.keep_alive))
            .clean_session(true)
            .finalize()
    };
    // Connect with default options and wait for it to complete or fail
    debug!("Connecting to the MQTT server");
    cli.connect(Some(conn_opts)).await?;

    Ok(cli)
}

/// Collects all results and prints them as JSON to the screen
async fn collect_and_print_results(
    devices: &Vec<AppDevice>,
    manager: &Manager,
    seconds_to_scan: u64,
) -> Result<(), Box<dyn Error>> {
    debug!("Start collecting data ...");

    // MAC adresses to check for ThermoBeacon devices
    let macs = &devices
        .iter()
        .map(|f| f.mac.parse::<BDAddr>().unwrap() as BDAddr)
        .collect();
    let results =
        thermobeacon_protocol::read_all_configured(manager, &macs, seconds_to_scan).await?;

    debug!(
        "Data collected. Found {} of {} devices.",
        results.len(),
        devices.len()
    );

    for result in results.into_iter() {
        let device = devices
            .iter()
            .find(|it| it.mac.parse::<BDAddr>().unwrap() == result.mac)
            .unwrap();

        info!("ThermoBeacon data: {:?}", result);

        let msg = Message {
            data: result,
            name: device.name.clone(),
        };
        println!("{}", serde_json::to_string(&msg).unwrap());
    }

    Ok(())
}

/// Collects all results and sends them to the given MQTT client
async fn collect_and_send_results(
    cli: &AsyncClient,
    devices: &Vec<AppDevice>,
    manager: &Manager,
    seconds_to_scan: u64,
) -> Result<(), Box<dyn Error>> {
    debug!("Start collecting data ...");

    // MAC adresses to check for ThermoBeacon devices
    let macs = &devices
        .iter()
        .map(|f| f.mac.parse::<BDAddr>().unwrap() as BDAddr)
        .collect();

    // Collect data from these MAC addresses
    let results =
        thermobeacon_protocol::read_all_configured(manager, &macs, seconds_to_scan).await?;

    debug!(
        "Data collected. Found {} of {} devices.",
        results.len(),
        devices.len()
    );

    for result in results.into_iter() {
        let device = devices
            .iter()
            .find(|it| it.mac.parse::<BDAddr>().unwrap() == result.mac)
            .unwrap();

        info!("ThermoBeacon data: {:?}", result);

        let msg = Message {
            data: result,
            name: device.name.clone(),
        };

        let topic = &device
            .topic
            .clone()
            .unwrap_or(format!("ThermoBeacon/{}", device.name));
        let qos = device.qos.unwrap_or(1);

        // Json message
        let payload = serde_json::to_string(&msg).unwrap();
        let msg = if device.retained {
            mqtt::Message::new(topic, payload, qos)
        } else {
            mqtt::Message::new_retained(topic, payload, qos)
        };
        cli.publish(msg).await?;
    }

    Ok(())
}

/// Executes the actual job: Check mqtt config, connect if possible else just print the results.
async fn job(config: &AppConfig, manager: &Manager) -> Result<(), Box<dyn Error>> {
    let client = match &config.mqtt {
        Some(mqtt_config) => match &mqtt_config.url {
            Some(_) => connect_to_mqtt(mqtt_config).await,
            None => Err("No MQTT configuration found".into()),
        },
        None => Err("No MQTT configuration found".into()),
    };
    match client {
        Ok(c) => {
            collect_and_send_results(&c, &config.devices, manager, config.seconds_to_scan).await?;

            c.disconnect(None).await?;
        }
        Err(_) => {
            warn!("No valid mqtt configuration found. Results are just printed to the console");
            collect_and_print_results(&config.devices, manager, config.seconds_to_scan).await?;
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    pretty_env_logger::init();

    let config: AppConfig = read_configuration();
    // Single instance to prevent D-Bus error: The maximum number of active connections for UID 0 has been reached
    let manager = Manager::new().await?;

    debug!("config {:?}", &config);

    if config.health.active {
        let ip = config.health.ip.as_str();
        let port = config.health.port;
        start_healthcheck_server(ip.to_string(), port).await?;
        info!(
            "Started health check service at http://{}:{}/health",
            ip, port
        );
    } else {
        debug!("Health check server not active");
    }

    if config.cron.is_some() {
        // There is some cron expression present, so we execute the job at a regular interval. Also check for a timezone to correctly calculate next execution.
        let cron_str = config.cron.clone().unwrap();

        info!("Execute job with cron expressions {}", &cron_str);

        let timezone_str = config.timezone.as_ref().unwrap();
        let timezone: chrono_tz::Tz = timezone_str
            .parse()
            .unwrap_or_else(|_| DEFAULT_TIMEZONE.to_string().parse().unwrap());
        loop {
            // Calculate the time of the next run (using the configured timezone)
            let now = Utc::now().with_timezone(&timezone);

            let next = cron_parser::parse(&cron_str, &now).unwrap();
            let dur = next.signed_duration_since(now).to_std().unwrap();

            let instant = tokio::time::Instant::now() + dur;

            info!("Next job execution {:?}", next);
            // Sleep until the next run
            let s = tokio::time::sleep_until(instant);
            // FInally execute run
            match job(&config, &manager).await {
                Ok(()) => {
                    let mut status = SYSTEM_STATUS.lock().unwrap();
                    *status = HealthStatus::Ok;
                    debug!("Run was successful");
                }
                Err(e) => {
                    let mut status = SYSTEM_STATUS.lock().unwrap();
                    *status = HealthStatus::LastRunFailed(e.to_string());
                    error!(
                        "Failed to read and deliver data, trying again next time: {:?}",
                        e
                    );
                }
            };
        }
    } else {
        info!("No cron descriptor found -> job is executed just once!");
        match job(&config, &manager).await {
            Ok(()) => {
                let mut status = SYSTEM_STATUS.lock().unwrap();
                *status = HealthStatus::Ok;
                debug!("Run was successful");
            }
            Err(e) => {
                let mut status = SYSTEM_STATUS.lock().unwrap();
                *status = HealthStatus::LastRunFailed(e.to_string());
                error!("Failed to read and deliver data: {:?}", e);
            }
        };
    }
    Ok(())
}
