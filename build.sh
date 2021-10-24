#!/bin/bash

set -o errexit
set -o nounset
set -o pipefail
set -o xtrace

readonly SOURCE_PATH=/target/release/frust
readonly TARGET_HOST=pi@raspberrypi.local
readonly TARGET_PATH=/home/pi/
docker buildx build -t frust --platform linux/arm/v7 .
docker run --platform linux/arm/v7 --rm --mount type=bind,source="$(pwd)",target="/build" frust cargo build --release
rsync ${SOURCE_PATH} ${TARGET_HOST}:${TARGET_PATH}
ssh -t ${TARGET_HOST} ${TARGET_PATH}