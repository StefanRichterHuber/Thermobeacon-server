extern crate paho_mqtt as mqtt;
extern crate pretty_env_logger;
#[macro_use]
extern crate log;

mod configuration;
mod health_check_server;
mod homeassistant;
mod thermobeacon_protocol;

use btleplug::{api::BDAddr, platform::Manager};
use chrono::Utc;
use configuration::{AppDevice, MqttConfig};
use mqtt::AsyncClient;

use std::{error::Error, time::Duration};

use crate::{
    configuration::{read_configuration, AppConfig, DEFAULT_TIMEZONE},
    health_check_server::{set_health_status, start_healthcheck_server, HealthStatus},
};

/// Structure of MQTT message send
#[derive(Debug, Default, serde_derive::Serialize, PartialEq)]
struct Message {
    data: thermobeacon_protocol::ThermoBeaconFullReadResult,
    name: String,
}

/// Tries to connect to the MQTT server using the given MqttConfig
pub async fn connect_to_mqtt(
    mqtt_config: &MqttConfig,
) -> Result<AsyncClient, Box<dyn Error + Send + Sync>> {
    // Create the client
    let cli = mqtt::AsyncClient::new(mqtt_config.url.clone().unwrap()).unwrap();

    let conn_opts = if mqtt_config.password.is_some() && mqtt_config.username.is_some() {
        debug!(
            "Configuration of MQTT with user {} and password ***",
            mqtt_config.username.clone().unwrap()
        );

        mqtt::ConnectOptionsBuilder::new_v5()
            .keep_alive_interval(Duration::from_secs(mqtt_config.keep_alive))
            .user_name(mqtt_config.username.clone().unwrap())
            .password(mqtt_config.password.clone().unwrap())
            .finalize()
    } else {
        debug!("Configuration of MQTT without username / password");
        mqtt::ConnectOptionsBuilder::new_v5()
            .keep_alive_interval(Duration::from_secs(mqtt_config.keep_alive))
            .finalize()
    };
    // Connect with default options and wait for it to complete or fail
    debug!("Connecting to the MQTT server");
    cli.connect(Some(conn_opts)).await?;

    Ok(cli)
}

/// Collects all results and prints them as JSON to the screen
async fn collect_and_print_results(
    devices: &[AppDevice],
    manager: &Manager,
    seconds_to_scan: u64,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    debug!("Start collecting data ...");

    // MAC adresses to check for ThermoBeacon devices
    let macs: Vec<BDAddr> = devices
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
    client: &AsyncClient,
    devices: &[AppDevice],
    manager: &Manager,
    seconds_to_scan: u64,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    debug!("Start collecting data ...");
    // MAC addresses to check for ThermoBeacon devices
    let macs: Vec<BDAddr> = devices
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
        client.publish(msg).await?;
    }

    Ok(())
}

/// Executes the actual job: Check mqtt config, connect if possible else just print the results.
async fn job(
    config: &AppConfig,
    manager: &Manager,
    client: &Option<AsyncClient>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    match client {
        Some(c) => {
            collect_and_send_results(c, &config.devices, manager, config.seconds_to_scan).await?;
        }
        None => {
            warn!("No valid mqtt configuration found. Results are just printed to the console");
            collect_and_print_results(&config.devices, manager, config.seconds_to_scan).await?;
        }
    }
    Ok(())
}

/// Executes the job using the configured cron schedule
async fn run_scheduled(
    manager: Manager,
    config: AppConfig,
    client: Option<AsyncClient>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
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
        tokio::time::sleep_until(instant).await;
        // Finally execute run
        match job(&config, &manager, &client).await {
            Ok(()) => {
                set_health_status(HealthStatus::Ok);
                debug!("Run was successful");
            }
            Err(e) => {
                set_health_status(HealthStatus::LastRunFailed(e.to_string()));
                error!(
                    "Failed to read and deliver data, trying again next time: {:?}",
                    e
                );
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    pretty_env_logger::init();

    let config = read_configuration();
    // Single instance to prevent D-Bus error: The maximum number of active connections for UID 0 has been reached
    let manager = Manager::new().await?;

    debug!("config {:?}", &config);

    let client = if let Some(mqtt_config) = &config.mqtt {
        let client = connect_to_mqtt(mqtt_config).await;
        match client {
            Ok(c) => Some(c),
            Err(e) => {
                error!("Failed to connect to MQTT server: {}", e);
                None
            }
        }
    } else {
        info!("No MQTT configuration found");
        None
    };

    // If an mqtt client is available and homea
    if let Some(cli) = &client {
        if let Some(mqtt_config) = &config.mqtt {
            if mqtt_config.homeassistant {
                info!("Home Assistant auto-discovery enabled!");
                homeassistant::publish_homeassistant_device_discovery_messages(&config, cli)
                    .await?;
            }
        }
    }

    if config.cron.is_some() {
        // Only start healthcheck server in cron jobs runs
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
        tokio::spawn(run_scheduled(manager, config, client))
            .await?
            .unwrap();
    } else {
        info!("No cron descriptor found -> job is executed just once!");
        match job(&config, &manager, &client).await {
            Ok(()) => {
                set_health_status(HealthStatus::Ok);
                debug!("Run was successful");
            }
            Err(e) => {
                set_health_status(HealthStatus::LastRunFailed(e.to_string()));
                error!("Failed to read and deliver data: {:?}", e);
            }
        };
    }
    Ok(())
}
