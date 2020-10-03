#!/bin/sh

# ALAAAAARM
MIN_TEMP=3300
MAX_TEMP=5000
PHONE=

N_RESTARTS=$(systemctl show frust.service -p NRestart)
LAST_SMS=$(date +%s)
while :
do
  echo "Checking for anomalies"

  RESTARTS=$(systemctl show frust.service -p NRestarts | sed -r 's/NRestarts=([0-9]*)/\1/g')
  if [ "$RESTARTS" -gt "0" ];
  then
    if [ "$LAST_SMS" -lt $(date -d '-5 minutes' +%s) ];
    then
      echo "Detected a restart. Sending SMS"
      /home/pi/.local/bin/aws sns publish \
        --message "Too many restarts detected. Please take a look!" \
        --phone-number $PHONE
      LAST_SMS=$(date +%s)
    else
      echo "Not sending an SMS. Too spammy"
    fi
  fi

  # Check temperature is not crazy
  INSIDE=$(cat /sys/bus/w1/devices/10-0008039a5582/w1_slave | grep -oP 't=\K([0-9]*)')
  if [ "$INSIDE" -lt "$MIN_TEMP" ];
  then
    if [ "$LAST_SMS" -lt $(date -d '-5 minutes' +%s) ];
    then
      echo "Warning, temperature $INSIDE is way too low! Sending SMS"
      /home/pi/.local/bin/aws sns publish \
          --message "Temperature is below 3.3 degrees!" \
          --phone-number $PHONE
        LAST_SMS=$(date +%s)
    fi
  fi

  if [ "$INSIDE" -gt "$MAX_TEMP" ];
  then
    if [ "$LAST_SMS" -lt $(date -d '-5 minutes' +%s) ];
    then
      echo "Warning, temperature $INSIDE is way too high! Sending SMS"
      /home/pi/.local/bin/aws sns publish \
          --message "Temperature is above 5 degrees!" \
          --phone-number $PHONE
        LAST_SMS=$(date +%s)
    fi
  fi

  sleep 5
done