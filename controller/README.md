# frust

Fridge controller written in Rust.

## Build

```
cargo build
```

## Run

There should be two DS18B20 temperature sensors. The sensors are exposed via a 1-Wire interface. I followed [this](https://www.circuitbasics.com/raspberry-pi-ds18b20-temperature-sensor-tutorial/) tutorial to export the two sensors.

Once setup run.

```
sudo INSIDE_SENSOR=/sys/bus/w1/devices/10-0008039a5582/w1_slave OUTSIDE_SENSOR=/sys/bus/w1/devices/10-0008039e9723/w1_slave target/debug/frust
```
