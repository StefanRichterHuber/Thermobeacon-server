use config::Config;
use dotenv::dotenv;
use std::env;

/// Configuration of the MQTT connection
#[derive(Debug, Clone, Default, serde_derive::Deserialize, PartialEq, Eq)]
pub struct MqttConfig {
    /// URL of the MQTT server
    pub url: Option<String>,
    #[serde(rename(deserialize = "keepAlive"), default = "default_keep_alive")]
    /// Keep alive time of the connection to the server
    pub keep_alive: u64,
    #[serde(rename(deserialize = "username"))]
    /// Optional username for the mqtt server
    pub username: Option<String>,
    /// Optional password for the mqtt server
    pub password: Option<String>,
    /// Optional password file for the mqtt server password
    pub password_file: Option<String>,
    /// Optional support for Home assistant
    #[serde(default)]
    pub homeassistant: bool,
}

/// Default keep_alive value
fn default_keep_alive() -> u64 {
    60
}

/// Configuration of a single known ThermoBeacon device
#[derive(Debug, Clone, Default, serde_derive::Deserialize, PartialEq, Eq)]
pub struct AppDevice {
    /// BLE MAC of the device
    pub mac: String,
    /// Human-readable name of the device (for the MQTT message)
    pub name: String,
    /// Topic of the MQTT message
    pub topic: Option<String>,
    /// QOS level of the MQTT message
    pub qos: Option<i32>,
    /// Should  the message be retained by the broker?
    #[serde(default)]
    pub retained: bool,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
}

/// Configuration of the health check
#[derive(Debug, Clone, serde_derive::Deserialize, PartialEq, Eq)]
pub struct HealthCheckConfig {
    /// Health check active
    #[serde(default)]
    pub active: bool,
    /// IP bind for health check service, defaults to "127.0.0.1"
    #[serde(default = "default_server_ip")]
    pub ip: String,
    /// Port of the health check service,defaults to 8080
    #[serde(default = "default_server_port")]
    pub port: u16,
}

/// Default server ip
fn default_server_ip() -> String {
    "127.0.0.1".to_string()
}

/// Default server port
fn default_server_port() -> u16 {
    8080
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        HealthCheckConfig {
            active: Default::default(),
            ip: default_server_ip(),
            port: default_server_port(),
        }
    }
}

/// Main configuration structure
#[derive(Debug, Clone, Default, serde_derive::Deserialize, PartialEq, Eq)]
pub struct AppConfig {
    /// List of devices to read values from
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub devices: Vec<AppDevice>,
    /// CRON expression for the poll interval
    pub cron: Option<String>,
    /// Timezone for the CRON expression
    pub timezone: Option<String>,
    /// MQTT client configuration
    pub mqtt: Option<MqttConfig>,
    /// Time in seconds to scan for devices
    pub seconds_to_scan: u64,
    /// Health check options
    #[serde(default)]
    pub health: HealthCheckConfig,
}

/// Timezone assumed if none configured
pub static DEFAULT_TIMEZONE: &str = "UTC";

/// Read the configuration
pub fn read_configuration() -> AppConfig {
    dotenv().ok();
    let settings = Config::builder()
        // Add optional file source `./config.yml"
        .add_source(config::File::with_name("config").required(false))
        // Add in settings from the environment (with a prefix of APP)
        // Eg.. `APP_DEBUG=1 ./target/app` would set the `debug` key.  APP_MQTT_PASSWORD would set mqtt.password key.
        .add_source(config::Environment::with_prefix("APP").separator("_"))
        .set_default("seconds_to_scan", 30)
        .unwrap()
        .build()
        .unwrap();

    let mut config: AppConfig = settings.try_deserialize().unwrap();

    // Check if we have to load the password file. If it is present, load its content and place it into password field of the mqtt config
    if config
        .mqtt
        .as_ref()
        .map(|c| c.password.is_none() && c.password_file.is_some())
        .unwrap_or(false)
    {
        let mqttconfig = config.mqtt.unwrap();
        let file = mqttconfig.password_file.as_ref().unwrap();

        let filepw = std::fs::read_to_string(file);

        config = match filepw {
            Ok(pw) => AppConfig {
                mqtt: Some(MqttConfig {
                    password: Some(pw),
                    ..mqttconfig
                }),
                ..config
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
            timezone: Some(timezone),
            ..config
        };
    }

    config
}
