extern crate paho_mqtt as mqtt;
extern crate pretty_env_logger;

use btleplug::api::{BDAddr, Central, Manager as _, Peripheral, PeripheralProperties, ScanFilter};
use btleplug::platform::Manager;
use std::error::Error;
use std::time::Duration;
use tokio::time::{self};
// Prelude import with the common imports
use packed_struct::prelude::*;

/// Raw data from ThermoBeacon. Struct uses PackedStruct to parse a byte array
/// @see https://github.com/iskalchev/ThermoBeacon-pyhap
///
/// Message length: 20 bytes
/// bytes | content
/// ========================================================
/// 00-01 | code
/// 02-02 | 00 ?
/// 03-03 | 0x80 if Button is pressed else 00
/// 04-09 | mac address
/// 10-11 | battery level: seems that 3400 = 100% (3400 mV, not quite sure)
/// 12-13 | temperature
/// 14-15 | humidity
/// 16-19 | uptime: seconds sinse the last reset

#[derive(PackedStruct)]
#[packed_struct(bit_numbering = "msb0", endian = "lsb")]
#[allow(dead_code)]
pub struct ThermoBeaconRawData {
    #[packed_field(bytes = 0)]
    unknown: u8,
    #[packed_field(bytes = 1)]
    button: u8,
    #[packed_field(bytes = "2..=7")]
    mac: u64,
    #[packed_field(bytes = 8)]
    voltage_raw: u16,
    #[packed_field(bytes = 10)]
    temperature_raw: u16,
    #[packed_field(bytes = 12)]
    humidity_raw: u16,
    #[packed_field(bytes = 14)]
    uptime_seconds: u32,
}

/// Raw data from ThermoBeacon containg the min / max temperature. Struct uses PackedStruct to parse a byte array
/// @see https://github.com/iskalchev/ThermoBeacon-pyhap
///
/// Message length: 22 bytes
/// bytes | content
/// ========================================================
/// 00-01 | code
/// 02-02 | 00 ?
/// 03-03 | 0x80 if Button is pressed else 00
/// 04-09 | mac address
/// 10-11 | max temp
/// 12-15 | max temp time (s)
/// 16-17 | min temp
/// 18-21 | min temp time (s)

#[derive(PackedStruct)]
#[packed_struct(bit_numbering = "msb0", endian = "lsb")]
#[allow(dead_code)]
pub struct ThermoBeaconMinMaxRawData {
    #[packed_field(bytes = 0)]
    unknown: u8,
    #[packed_field(bytes = 1)]
    button: u8,
    #[packed_field(bytes = "2..=7")]
    mac: u64,
    #[packed_field(bytes = 8)]
    max_temperature_raw: u16,
    #[packed_field(bytes = 10)]
    max_temp_time_seconds: u32,
    #[packed_field(bytes = 14)]
    min_temperature_raw: u16,
    #[packed_field(bytes = 16)]
    mintemp_time_seconds: u32,
}

/// Struct containing the parsed data from a ThermoBeacon
#[allow(dead_code)]
#[derive(Debug, Default, Clone, serde_derive::Serialize, PartialEq)]
pub struct ThermoBeaconData {
    /// Battery level (0 - 100%)
    battery_level: f32,
    /// Humidity (0 - 100%)
    humidity: f32,
    /// Temperature (°C)
    temperature: f32,
    /// Uptime in s
    uptime_s: u32,
    /// Updtime in days
    uptime_d: f32,
    /// Mac Adress of the ThermoBeacon
    mac: BDAddr,
    /// Is the button currently pressed?
    button_pressed: bool,
}

/// Struct containing the parsed min/max data from a ThermoBeacon
#[allow(dead_code)]
#[derive(Debug, Default, Clone, serde_derive::Serialize, PartialEq)]
pub struct ThermoBeaconMinMaxData {
    /// Is the button currently pressed?
    button_pressed: bool,
    /// Mac Adress of the ThermoBeacon
    mac: BDAddr,
    /// max. temperature (°C)
    max_temperature: f32,
    // min. temperature (°C)
    min_temperature: f32,
    // time of max temperature (relative to start time)
    max_temp_time: u32,
    // time of min temperature  (relative to start time)
    min_temp_time: u32,
}

/// Allows to convert the ThermoBeaconRawData to a ThermoBeaconData struct.
impl From<ThermoBeaconRawData> for ThermoBeaconData {
    fn from(value: ThermoBeaconRawData) -> Self {
        // https://github.com/iskalchev/ThermoBeacon-pyhap/blob/main/tb_protocol.py
        let t = value.temperature_raw as f32 / 16.0;
        let h = value.humidity_raw as f32 / 16.0;

        ThermoBeaconData {
            battery_level: value.voltage_raw as f32 * 100.0 / 3400.0,
            humidity: if h > 4000.0 { h - 4096.0 } else { h },
            temperature: if t > 4000.0 { t - 4096.0 } else { t },
            uptime_s: value.uptime_seconds,
            uptime_d: value.uptime_seconds as f32 / 86400.0,
            mac: value.mac.try_into().unwrap(),
            button_pressed: value.button == 0x80,
        }
    }
}

/// Allows to convert the ThermoBeaconMinMaxRawData to a ThermoBeaconMinMaxData struct.
impl From<ThermoBeaconMinMaxRawData> for ThermoBeaconMinMaxData {
    fn from(value: ThermoBeaconMinMaxRawData) -> Self {
        let t_max = value.max_temperature_raw as f32 / 16.0;
        let t_min = value.min_temperature_raw as f32 / 16.0;

        ThermoBeaconMinMaxData {
            button_pressed: value.button == 0x80,
            mac: value.mac.try_into().unwrap(),
            max_temperature: if t_max > 4000.0 {
                t_max - 4096.0
            } else {
                t_max
            },
            min_temperature: if t_min > 4000.0 {
                t_min - 4096.0
            } else {
                t_min
            },
            max_temp_time: value.max_temp_time_seconds,
            min_temp_time: value.mintemp_time_seconds,
        }
    }
}

/// Returns the length of the manufacturer_data field
fn get_property_length(properties: &PeripheralProperties) -> usize {
    for key in properties.manufacturer_data.keys() {
        return match key {
            key if check_if_device_type_is_valid(key) => {
                properties.manufacturer_data.get(key).unwrap().len()
            }
            _ => 0,
        };
    }
    return 0;
}

/// Checks if the device type is valid
fn check_if_device_type_is_valid(key: &u16) -> bool {
    match key {
        // Allowed values 0x10, 0x11, 0x15, 0x1B -> Different for different device types. 0x15 for Thermobeacon rounded corne with display
        0x10 | 0x11 | 0x15 | 0x1B => true,
        _ => false,
    }
}

/// Parses the current temperature and humidity data from PeripheralProperties
fn parse_thermo_beacon_data(p: &PeripheralProperties) -> Result<ThermoBeaconData, Box<dyn Error>> {
    trace!("  ThermoBeacon properties {:?}", p);
    for key in p.manufacturer_data.keys() {
        match key {
            key if check_if_device_type_is_valid(key) => {
                // Read the data
                let data = p.manufacturer_data.get(&key).unwrap();
                trace!("  Fetched {:?} bytes of raw data", data.len());

                if data.len() == 18 {
                    let tbrd: ThermoBeaconData = ThermoBeaconRawData::unpack(
                        data[0..18].try_into().expect("slice with incorrect length"),
                    )?
                    .into();

                    return Ok(tbrd);
                } else {
                    warn!("  Data length not 18 but {:?}", data.len());
                }
            }
            _ => warn!("  Device ID not supported {:?}", key),
        }
    }
    Err("No data found".into())
}

/// Parses the min and max temperature data from PeripheralProperties
fn parse_thermo_beacon_min_max_data(
    p: &PeripheralProperties,
) -> Result<ThermoBeaconMinMaxData, Box<dyn Error>> {
    trace!("  ThermoBeacon properties {:?}", p);
    for key in p.manufacturer_data.keys() {
        match key {
            key if check_if_device_type_is_valid(key) => {
                // Read the data
                let data = p.manufacturer_data.get(&key).unwrap();
                trace!("  Fetched {:?} bytes of raw data", data.len());

                if data.len() == 20 {
                    let tbrd: ThermoBeaconMinMaxData = ThermoBeaconMinMaxRawData::unpack(
                        data[0..20].try_into().expect("slice with incorrect length"),
                    )?
                    .into();

                    return Ok(tbrd);
                } else {
                    warn!("  Data length not 18 but {:?}", data.len());
                }
            }
            _ => warn!("  Device ID not supported {:?}", key),
        }
    }
    Err("No data found".into())
}

#[derive(Debug, Default, serde_derive::Serialize, PartialEq)]
pub struct ThermoBeaconFullReadResult {
    /// Battery level (0 - 100%)
    pub battery_level: f32,
    /// Humidity (0 - 100%)
    pub humidity: f32,
    /// Temperature (°C)
    pub temperature: f32,
    /// Uptime in s
    pub uptime: u32,
    /// Is the button currently pressed?
    pub button_pressed: bool,
    /// Mac Adress of the ThermoBeacon
    pub mac: BDAddr,
    /// max. temperature (°C)
    pub max_temperature: f32,
    // min. temperature (°C)
    pub min_temperature: f32,
    // time of max temperature (relative to start time)
    pub max_temp_time: u32,
    // time of min temperature  (relative to start time)
    pub min_temp_time: u32,
}

/// Reads all possible available data for the configured devices
pub async fn read_all_configured(
    manager: &Manager,
    devices: &Vec<BDAddr>,
    seconds_to_scan: u64,
) -> Result<Vec<ThermoBeaconFullReadResult>, Box<dyn Error>> {
    let time_to_wait_between_scans = 5;
    let adapter_list = manager.adapters().await?;
    if adapter_list.is_empty() {
        error!("No Bluetooth adapters found");
        return Err("No Bluetooth adapters found".into());
    }

    let mut result: Vec<ThermoBeaconFullReadResult> = vec![];
    for adapter in adapter_list.iter() {
        debug!("Starting scan on {}...", adapter.adapter_info().await?);
        adapter
            .start_scan(ScanFilter::default())
            .await
            .expect("Can't scan BLE adapter for connected devices...");
        time::sleep(Duration::from_secs(seconds_to_scan)).await;
        let peripherals = adapter.peripherals().await?;
        if peripherals.is_empty() {
            error!("->>> BLE peripheral devices were not found, sorry. Exiting...");
        } else {
            // All peripheral devices in range
            for peripheral in peripherals.iter() {
                let device = devices.iter().find(|d| peripheral.address() == **d);

                match device {
                    Some(_d) => {
                        let properties = peripheral.properties().await?;

                        if properties.is_some() {
                            let mut props = properties.unwrap();
                            let local_name = props
                                .clone()
                                .local_name
                                .unwrap_or(String::from("(peripheral name unknown)"));

                            if local_name == "ThermoBeacon" {
                                let measurement = match get_property_length(&props) {
                                    18 => {
                                        // Temperature and humdity data is available
                                        debug!(
                                        "Reading temperature and humidity from ThermoBeacon {:?}",
                                        peripheral.address()
                                    );
                                        let data = parse_thermo_beacon_data(&props)?;

                                        // Wait for the min_max data
                                        while get_property_length(&props) != 20 {
                                            time::sleep(Duration::from_secs(
                                                time_to_wait_between_scans,
                                            ))
                                            .await;
                                            props = match peripheral.properties().await? {
                                                Some(p) => p,
                                                None => props,
                                            }
                                        }
                                        debug!(
                                        "Reading min and max temperature from ThermoBeacon {:?}",
                                        peripheral.address()
                                    );
                                        let min_max_data =
                                            parse_thermo_beacon_min_max_data(&props)?;

                                        Some((data, min_max_data))
                                    }
                                    20 => {
                                        // Min-max data is available
                                        debug!(
                                        "Reading min and max temperature from ThermoBeacon {:?}",
                                        peripheral.address()
                                    );
                                        let min_max_data =
                                            parse_thermo_beacon_min_max_data(&props)?;

                                        // Wait  temperature and humidity data
                                        while get_property_length(&props) != 18 {
                                            time::sleep(Duration::from_secs(
                                                time_to_wait_between_scans,
                                            ))
                                            .await;
                                            props = match peripheral.properties().await? {
                                                Some(p) => p,
                                                None => props,
                                            }
                                        }
                                        debug!(
                                        "Reading temperature and humidity from ThermoBeacon {:?}",
                                        peripheral.address()
                                    );
                                        let data = parse_thermo_beacon_data(&props)?;

                                        Some((data, min_max_data))
                                    }
                                    _ => None,
                                };

                                match measurement {
                                    Some((data, min_max_data)) => {
                                        let r = ThermoBeaconFullReadResult {
                                            battery_level: data.battery_level,
                                            humidity: data.humidity,
                                            temperature: data.temperature,
                                            uptime: data.uptime_s,
                                            button_pressed: data.button_pressed,
                                            mac: data.mac,
                                            max_temperature: min_max_data.max_temperature,
                                            min_temperature: min_max_data.min_temperature,
                                            max_temp_time: min_max_data.max_temp_time,
                                            min_temp_time: min_max_data.min_temp_time,
                                        };

                                        result.push(r);
                                    }
                                    None => {}
                                }
                            }
                        }
                    }
                    None => {}
                }
            }
        }
        adapter.stop_scan().await?;
    }
    return Ok(result);
}
