#!/bin/bash
cargo build
OUTSIDE_SENSOR="/sys/bus/w1/devices/10-0008039e9723/w1_slave" INSIDE_SENSOR="/sys/bus/w1/devices/10-0008039a5582/w1_slave" target/debug/frust