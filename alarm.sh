#!/bin/sh

# ALAAAAARM
MIN_TEMP=2000
MAX_TEMP=5000

N_RESTARTS=$(systemctl show frust.service -p NRestarts | sed -r 's/NRestarts=([0-9]*)/\1/g')
LAST_SMS=0

send_sms() {
  if [ "$LAST_SMS" -lt $(date -d '-5 minutes' +%s) ];
  then
    echo "Sending SMS!"
    /home/pi/.local/bin/aws sns publish --message "$1" --phone-number "$PHONE"
    LAST_SMS=$(date +%s)
  else
    echo "Not sending an SMS. Too spammy"
  fi
}

while :
do
  echo "Checking for anomalies"
  echo "Last time: $LAST_SMS"
  echo "Current: $(date -d '-5 minutes' +%s)"

  RESTARTS=$(systemctl show frust.service -p NRestarts | sed -r 's/NRestarts=([0-9]*)/\1/g')
  if [ "$RESTARTS" -gt "$N_RESTARTS" ];
  then
    echo "Detected a restart. Sending SMS"
    send_sms "Controller is restarting. Please take a look what happened!"
    N_RESTARTS=$(systemctl show frust.service -p NRestarts | sed -r 's/NRestarts=([0-9]*)/\1/g')
  fi

  # Check temperature is not crazy
  INSIDE=$(cat /sys/bus/w1/devices/10-0008039a5582/w1_slave | grep -oP 't=\K([0-9]*)')
  if [ "$INSIDE" -lt "$MIN_TEMP" ];
  then
    echo "Warning, temperature $INSIDE is way too low! Sending SMS"
    send_sms "Temperature is below $MIN_TEMP degrees!"
  fi

  if [ "$INSIDE" -gt "$MAX_TEMP" ];
  then
    echo "Warning, temperature $INSIDE is way too high! Sending SMS"
    send_sms "Temperature is above $MAX_TEMP degrees!"
  fi

  sleep 5
done
