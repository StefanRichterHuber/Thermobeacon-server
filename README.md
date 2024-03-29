[![Build](https://github.com/StefanRichterHuber/Thermobeacon-server/actions/workflows/docker-image.yml/badge.svg)](https://github.com/StefanRichterHuber/Thermobeacon-server/actions/workflows/docker-image.yml)

# Thermobeacon-server

This applications uses BLE to scan for ThermoBeacon smart hygrometers and publish the available data to a MQTT server. Its especially designed to work within docker. It has build-in support for Home Assistant auto-discovery (for temperature, humidity and battery level).

## Previous work and motivation

I had several ThermoBeacon sensors distributed in my house, which I wanted to integrate into in my smart home setup. Since the current setup uses a MQTT broker to collect sensor data, MQTT was decided to be the target platform.

There are already at least two projects [thermobeacon](https://github.com/rnlgreen/thermobeacon ) and [ThermoBeacon-pyhap](https://github.com/iskalchev/ThermoBeacon-pyhap) available to access and parse the data from the ThermoBeacons. Both helped me to understand the protocol and write the parser.

Unfortunately both apps where not a perfect fit, especially when the deployment platform is docker. Python runtime results in relatively heavy-weight containers and both scripts do not support a proper configuration using environment variables (see [The twelve-factor app](https://12factor.net/) ).

## Requirements

Of course you need a host system with a bluetooth BLE adapter.
To build the project one can either install rust and following dependencies (for debian)

```bash
apt-get install -y libdbus-1-dev libssl-dev build-essential cmake
```

Then adapt the `config.yml` file and just  

```bash
cargo run
```

to start collecting data.

Or you can just run `docker pull stefanrichterhuber/thermobeaconserver:latest` or the given multistage `Dockerfile` to build an image suitable for your platform. Just rembember the container needs to be privileged (to access Bluetooth) and the necessary `/var/run/dbus/system_bus_socket` must be handed into the container.

## Configuration

This application uses the [config crate](https://docs.rs/config/latest/config/) for configuration. On startup it searches for a `config.yml` file. All config values

```yml
---
# Configuration of the thermobeacon-server
# To set config with environment variables use prefix APP and underscore separator.
# Examples: 
# APP_MQTT_URL -> mqtt.url
# APP_DEVICES[0]_NAME -> devices[0].name

devices: # List of devices to scan (can be multiple devices)
- mac: xx:xx:xx:xx:xx:xx #MAC of the BLE Thermobeacon. Can be fetched from the app.  Will be part of the MQTT message to identify the source. Required.
  name: Basement # Human readable name of the beacon. Will be part of the MQTT message to identify the source. Required.
  topic: home/ThermoBeacon/Basement # MQTT topic. Defaults to 'ThermoBeacon/{name}'
  manufacturer: Unknown # Optional device manufacturer for Home Assistant auto discovery. Defaults to 'Unknown'
  model: Smart hygrometer # Optional device model for Home Assistant auto discovery. Defaults to 'Smart hygrometer'
  retained: false # Should the latest MQTT message be retained by the broker? (Defaults to false)
cron: "*/1 * * * *" # CRON expression. If none given, the configured devices are only read once and the app stops immediately after.
seconds_to_scan: 30 # Seconds to scan for bluetooth devices. Defaults to 30s.
#timezone: Europe/Berlin # Timezone for parsing the CRON expression. Defaults to UTC.
mqtt:
  url: tcp://localhost:1883 # URL to MQTT
  #username: # Optional MQTT user. If not set, anonymous access to server is tried.
  #password: # Optional MQTT password. If not set, anonymous access to server is tried.
  #password_file # Optional File containing MQTT password (to use docker secrets)
  #homeassistant # Enable optional Home Assistant auto-discovery support. Defaults to false.
```

Alternatively the app can be configured using environment variables. Use the `APP_` prefix, the underscore separator and uppercase keys to generate the corresponding variable names. The app also supports using `.env` files.

```env
APP_DEVICES[0]_MAC=xx:xx:xx:xx:xx:xx
APP_DEVICES[0]_NAME=Basement
APP_DEVICES[0]_TOPIC=home/ThermoBeacon/Basement
APP_DEVICES[1]_MAC=xx:xx:xx:  xx:xx:xy
APP_DEVICES[1]_NAME=Kitchen
APP_DEVICES[1]_TOPIC=home/ThermoBeacon/Kitchen
APP_CRON=*/1 * * * *
APP_MQTT_URL=tcp://localhost:1883 
```

## MQTT message format

A JSON string is send to configured topic on the MQTT broker.

```json
{
    "data":{
        "battery_level":83.26471,
        "humidity":46.1875,
        "temperature":17.5625,
        "uptime":5090587,
        "button_pressed":false,
        "mac":"xx:xx:xx:xx:xx:xx",
        "max_temperature":24.9375,
        "min_temperature":12.75,
        "max_temp_time":4493928,
        "min_temp_time":5002144
    },
    "name":"Basement"
}
```

- `battery_level`: Battery level 0 - 100%
- `humidity`: Humidity 0 - 100%
- `temperature`: Current temperature (°C)
- `uptime`: Time in seconds since the last reset
- `button_pressed`: Is the connect button currently pressed?
- `mac`: BLE MAC of the device (see device configuration)
- `max_temperature`: Maximum temperature (°C) measured since last reset
- `max_temp_time`: Time in seconds from the last reset to the time the maximum temperature was read
- `min_temperature`: Minimum temperature (°C) measured since last reset
- `min_temp_time`:  Time in seconds from the last reset to the time the minimum temperature was read
- `name`: Given name of the device (see device configuration)

By subtracting the `uptime` from the current time, one can determine when the last reset of the sensor happened.
By subtracting `max_temp_time` or `min_temp_time` from `uptime`, one can determine how long ago the corresponding event happened.

## Deployment with docker

I recommend writing a `docker-compose.yml` file to properly configure the app.

```yml
# Example configuration for the usage of the thermobeacon server

version: '3.8'
services:
  mqtt: # MQTT broker to collect data
    image: eclipse-mosquitto:latest
    restart: unless-stopped
    volumes:
      - "./mosquitto-data:/mosquitto"
    ports:
      - "1883:1883"
      - "9001:9001"
    command: "mosquitto -c /mosquitto-no-auth.conf"

  node-red: # Node red server to process and visualize data 
    image: nodered/node-red:latest
      - TZ=Europe/Berlin
    ports:
      - "1880:1880"
    volumes:
      - node-red-data:/data

  thermobeacon: # Thermobeacon server to collect data from the thermobeacons.
    image: thermobeacon-server:latest
    restart: unless-stopped
    privileged: true # Necessary to have enough permissions to access dbus with bluetooth devices of the host
    environment:
      - TZ=Europe/Berlin # Optional set timezone for proper calculation of the next invocation from cron expression
      #- RUST_LOG=debug # Optional set debug level to error/warn/info/debug to resolve connection issues
      - APP_DEVICES[0]_MAC=xx:xx:xx:xx:xx:xx
      - APP_DEVICES[0]_NAME=Basement
      - APP_DEVICES[0]_TOPIC=home/ThermoBeacon/Basement
      - APP_DEVICES[1]_MAC=xx:xx:xx:xx:xx:xy
      - APP_DEVICES[1]_NAME=Kitchen
      - APP_DEVICES[1]_TOPIC=home/ThermoBeacon/Kitchen
      - APP_CRON=*/1 * * * *
      - APP_MQTT_URL=tcp://mqtt:1883 
    volumes:
      - /var/run/dbus/system_bus_socket:/var/run/dbus/system_bus_socket # Necessary to access the bluetooth devices of the host
      # - ./config.yml:/app/config.yml # Instead of using environment variables, one can also just map a config file

volumes:
  node-red-data: # Volume to persist Node red configuration
```

## Health check

There is a simple health check endpoint present. By default it is disabled, but it can be activated with either `APP_HEALTH_ACTIVE=true` or in the config file.

```yml
health:
  active: true
  ip: 127.0.0.1
  port: 8080

```

It provides a simple HTTP endpoint at `http://127.0.0.1:8080/health` which can be polled. It returns status code `404` until the first run, status code `200` for the first successful run and status code `500` if the last run failed.
The dockerfile includes `curl` so you could simply add a health check to your `docker-compose.yml`. Just ensure the interval matches your cron expression.

```yml
healthcheck:
  test: ["CMD", "curl", "-f", "http://127.0.0.1:8080/health"]
  interval: 1m
  timeout: 10s
  retries: 3
  start_period: 1m
```

## Architecture

In order to create a lightweight app, Rust was decided to use. Since the interaction with the selected crate to handle BLE ([bteplug](https://lib.rs/crates/btleplug) ) required an async runtime, the whole app is based on tokio.

 On startup the configuration is read once using [config crate](https://docs.rs/config/latest/config/). If a cron expression (parsed by [cron-parser](https://docs.rs/cron-parser/latest/cron_parser/)) is configured, a loop is entered which calculates the time of the next run based on the cron expression and the configured timezone (or UTC). Without cron expression, fetching and sending the data only happens once before the app quits. To send the data to the mqtt broker, [paho-mqtt](https://github.com/eclipse/paho.mqtt.rust) is used. If no valid mqtt connection is possible, the JSON document is just send to std out.

The actual handling of the protocol happens in `thermobeacon_protocol.rs`. Each ThermoBeacon device sends alternating messages to the `manufacturer_data` field. One message (identified by a length of 20 bytes) contains the current temperature / humidity / uptime and another message (identified by a length of 22 bytes) contains the minimum / maximum temperature and the time of these events.
For each configured device found, the app waits for both messages. This can take several seconds (up to 30s)! No pairing with the devices is necessary. Using [packed_struct](https://docs.rs/packed_struct/latest/packed_struct/) both raw messages are decoded, proccessed to calculate the real values, then combined into a single message with the given name of the device and send to the target.

First message with temperature / humidity / uptime. Message length is 20 bytes. Encoding of multibyte values is lsb. See [ThermoBeacon-pyhap](https://github.com/iskalchev/ThermoBeacon-pyhap).

| bytes | content |
| --- | --- |
| 00-01 | code (depends on the manufacturer of the devices, known values are 0x10, 0x11, 0x15 ) |
| 02-02 | 00 ? |
| 03-03 | 0x80 if Button is pressed else 00 |
| 4-09 | mac address |
| 10-11 | battery level: seems that 3400 = 100% (3400 mV, not quite sure) |
| 12-13 | temperature (divide by 16 to get actual temperature in °C. If value is greater than 4000, substract by 4096 to get negative temperatures) |
| 14-15 | humidity (divide by 16 to get actual humidity in %)|
| 16-19 | uptime: seconds sinse the last reset |

Second message with min / max temperature. Message length is 22 bytes. Encoding of multibyte values is lsb. See [ThermoBeacon-pyhap](https://github.com/iskalchev/ThermoBeacon-pyhap).

| bytes | content
| --- | --- |
| 00-01 | code (depends on the manufacturer of the devices, known values are 0x10, 0x11, 0x15 ) |
| 02-02 | 00 ? |
| 03-03 | 0x80 if Button is pressed else 00 |
| 04-09 | mac address |
| 10-11 | max temp (divide by 16 to get actual temperature in °C. If value is greater than 4000, substract by 4096 to get negative temperatures)|
| 12-15 | max temp time (s) |
| 16-17 | min temp (divide by 16 to get actual temperature in °C. If value is greater than 4000, substract by 4096 to get negative temperatures)|
| 18-21 | min temp time (s) |

Home Assistant auto-discovery is implemented by sending the corresponding MQTT [Discovery Messages](https://www.home-assistant.io/integrations/mqtt/#mqtt-discovery) at program startup for humidity, temperature and battery level using the hard-coded config topics: `homeassistant/sensor/thermobeacon/[device_mac with : replaced with _]_[temperature|humidity|battery]/config`. The state topic in the config references the configured topic for the device (e.g `ThermoBeacon/[device name]`). The server does not check if the configured device is reachable before announcing it to Home Assistant.
