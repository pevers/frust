#!/bin/bash

set -e
set -x

export INSIDE_SENSOR=test/mock_sensor
export OUTSIDE_SENSOR=test/mock_sensor
cargo run