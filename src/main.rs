extern crate paho_mqtt as mqtt;
extern crate pretty_env_logger;
#[macro_use]
extern crate log;

mod thermobeacon_protocol;

use btleplug::{api::BDAddr, platform::Manager};
use chrono::Utc;
use config::Config;
use mqtt::AsyncClient;
use std::{
    env::{self},
    error::Error,
    time::Duration,
};

/// Configuration of the MQTT connection
#[derive(Debug, Clone, Default, serde_derive::Deserialize, PartialEq, Eq)]
struct MqttConfig {
    /// URL of the MQTT server
    url: Option<String>,
    #[serde(rename(deserialize = "keepAlive"))]
    /// Keep alive time of the connection to the server
    keep_alive: u64,
    #[serde(rename(deserialize = "username"))]
    /// Optional username for the mqtt server
    username: Option<String>,
    /// Optional password for the mqtt server
    password: Option<String>,
    /// Optional password file for the mqtt server password
    password_file: Option<String>,
}

/// Configuration of a single known ThermoBeacon device
#[derive(Debug, Clone, Default, serde_derive::Deserialize, PartialEq, Eq)]
pub struct AppDevice {
    /// BLE MAC of the device
    mac: String,
    /// Human-readable name of the device (for the MQTT message)
    name: String,
    /// Topic of the MQTT message
    topic: Option<String>,
    /// QOS level of the MQTT message
    qos: Option<i32>,
    /// Should  the message be retained by the broker?
    #[serde(default)]
    retained: bool,
}

/// Main configuration structure
#[derive(Debug, Clone, Default, serde_derive::Deserialize, PartialEq, Eq)]
struct AppConfig {
    /// List of devices to read values from
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    devices: Vec<AppDevice>,
    /// CRON expression for the poll interval
    cron: Option<String>,
    /// Timezone for the CRON expression
    timezone: Option<String>,
    /// MQTT client configuration
    mqtt: Option<MqttConfig>,
    /// Time in seconds to scan for devices
    seconds_to_scan: u64,
}

/// Structure of MQTT message send
#[derive(Debug, Default, serde_derive::Serialize, PartialEq)]
struct Message {
    data: thermobeacon_protocol::ThermoBeaconFullReadResult,
    name: String,
}

/// Timezone assumed if none configured
static DEFAULT_TIMEZONE: &str = "UTC";

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

/// Read the configuration
fn read_configuration() -> AppConfig {
    let settings = Config::builder()
        // Add optional file source `./config.yml"
        .add_source(config::File::with_name("config").required(false))
        // Add in settings from the environment (with a prefix of APP)
        // Eg.. `APP_DEBUG=1 ./target/app` would set the `debug` key.  APP_MQTT_PASSWORD would set mqtt.password key.
        .add_source(config::Environment::with_prefix("APP").separator("_"))
        // Default connection keep alive of 20s
        .set_default("mqtt.keepAlive", 20)
        .unwrap()
        .set_default("seconds_to_scan", 30)
        .unwrap()
        .build()
        .unwrap();

    let mut config: AppConfig = settings.try_deserialize().unwrap();

    // Check if we have to load the password file. If it is present, load its content and place it into password field of the mqtt config
    if config
        .mqtt
        .as_ref()
        .and_then(|c| Some(c.password.is_none() && c.password_file.is_some()))
        .unwrap_or(false)
    {
        let mqttconfig = config.mqtt.as_ref().unwrap();
        let file = mqttconfig.password_file.as_ref().unwrap();

        let filepw = std::fs::read_to_string(&file);

        config = match filepw {
            Ok(pw) => AppConfig {
                devices: config.devices,
                cron: config.cron,
                timezone: config.timezone,
                seconds_to_scan: config.seconds_to_scan,
                mqtt: Some(MqttConfig {
                    url: mqttconfig.url.clone(),
                    keep_alive: mqttconfig.keep_alive,
                    username: mqttconfig.username.clone(),
                    password: Some(pw),
                    password_file: mqttconfig.password_file.clone(),
                }),
            },
            Err(e) => {
                error!(
                    "password_file {} configured, but not readable!: {:?}",
                    file, e
                );
                std::process::exit(1);
            }
        };
    }

    // Check if timezone for chron is configured. If not, read environment variable TZ. If no value found, use default timezone UTC to set config variable timezone.
    if config.timezone.is_none() {
        let timezone = env::var("TZ").unwrap_or(DEFAULT_TIMEZONE.to_string());

        config = AppConfig {
            devices: config.devices,
            seconds_to_scan: config.seconds_to_scan,
            cron: config.cron,
            mqtt: config.mqtt,
            timezone: Some(timezone),
        };
    }

    return config;
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    pretty_env_logger::init();

    let config: AppConfig = read_configuration();
    // Single instance to prevent D-Bus error: The maximum number of active connections for UID 0 has been reached
    let manager = Manager::new().await?;

    debug!("config {:?}", &config);

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
            tokio::time::sleep_until(instant).await;
            // FInally execute run
            match job(&config, &manager).await {
                Ok(()) => {
                    debug!("Run was successfull");
                }
                Err(e) => {
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
                debug!("Run was successfull");
            }
            Err(e) => {
                error!("Failed to read and deliver data: {:?}", e);
            }
        };
    }
    Ok(())
}
