#!/bin/bash

set -e
set -x

export INSIDE_SENSOR=/sys/devices/w1_bus_master1/10-0008039a5582/w1_slave
export OUTSIDE_SENSOR=/sys/devices/w1_bus_master1/10-0008039e9723/w1_slave
cargo run