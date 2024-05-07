use std::error::Error;

use paho_mqtt::AsyncClient;

/// Describes a device for automatic discovery of device topics
#[derive(Debug, Clone, Default, serde_derive::Serialize, PartialEq)]
pub struct MQTTDiscoveryDevice {
    pub identifiers: Vec<String>,
    pub name: String,
    pub manufacturer: String,
    pub model: String,
}

/// Describes the message send to 'homeassistant' topic for automatic discovery of device topics
#[derive(Debug, Clone, Default, serde_derive::Serialize, PartialEq)]
pub struct MQTTDiscovery {
    pub device_class: String,
    pub state_topic: String,
    pub unit_of_measurement: String,
    pub value_template: String,
    pub unique_id: String,
    pub device: MQTTDiscoveryDevice,
}

/// Sends the Home assistant auto discovery messages for all configured devices
pub async fn publish_homeassistant_device_discovery_messages(
    config: &crate::configuration::AppConfig,
    cli: &AsyncClient,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    for device in &config.devices {
        // https://www.home-assistant.io/integrations/mqtt/
        // https://www.home-assistant.io/integrations/sensor/
        // https://www.home-assistant.io/docs/configuration/customizing-devices/#device-class

        // State topic
        let topic = &device
            .topic
            .clone()
            .unwrap_or(format!("ThermoBeacon/{}", device.name));

        let topic_temperature = format!(
            "homeassistant/sensor/thermobeacon/{}_temperature/config",
            device.mac.replace(":", "_")
        );
        let device_id = MQTTDiscoveryDevice {
            identifiers: vec![device.mac.clone()],
            name: device.name.clone(),
            manufacturer: device
                .manufacturer
                .as_ref()
                .unwrap_or(&"Unknown".to_string())
                .to_string(),
            model: device
                .model
                .as_ref()
                .unwrap_or(&"Smart hygrometer".to_string())
                .to_string(),
        };

        let payload_temperature = MQTTDiscovery {
            device_class: "temperature".to_string(),
            state_topic: topic.clone(),
            unit_of_measurement: "°C".to_string(),
            value_template: "{{ value_json.data.temperature}}".to_string(),
            unique_id: format!("{}_temp", device.mac),
            device: device_id.clone(),
        };

        let topic_humidity = format!(
            "homeassistant/sensor/thermobeacon/{}_humidity/config",
            device.mac.replace(":", "_")
        );
        let payload_humidity = MQTTDiscovery {
            device_class: "humidity".to_string(),
            state_topic: topic.clone(),
            unit_of_measurement: "%".to_string(),
            value_template: "{{ value_json.data.humidity}}".to_string(),
            unique_id: format!("{}_humidity", device.mac),
            device: device_id.clone(),
        };

        let topic_battery = format!(
            "homeassistant/sensor/thermobeacon/{}_battery/config",
            device.mac.replace(":", "_")
        );
        let payload_battery = MQTTDiscovery {
            device_class: "battery".to_string(),
            state_topic: topic.clone(),
            unit_of_measurement: "%".to_string(),
            value_template: "{{ value_json.data.battery_level}}".to_string(),
            unique_id: format!("{}_battery", device.mac),
            device: device_id.clone(),
        };

        debug!(
            "Publish discovery message for temperature of {} to {}: {}",
            device.name,
            topic_temperature,
            serde_json::to_string(&payload_temperature).unwrap()
        );
        cli.publish(mqtt::Message::new(
            topic_temperature,
            serde_json::to_string(&payload_temperature).unwrap(),
            1,
        ))
        .await?;

        debug!(
            "Publish discovery message for humidity of {} to {}: {}",
            device.name,
            topic_humidity,
            serde_json::to_string(&payload_humidity).unwrap()
        );
        cli.publish(mqtt::Message::new(
            topic_humidity,
            serde_json::to_string(&payload_humidity).unwrap(),
            1,
        ))
        .await?;

        debug!(
            "Publish discovery message for battery level of {} to {}: {}",
            device.name,
            topic_battery,
            serde_json::to_string(&payload_battery).unwrap()
        );
        cli.publish(mqtt::Message::new(
            topic_battery,
            serde_json::to_string(&payload_battery).unwrap(),
            1,
        ))
        .await?;
    }
    Ok(())
}
