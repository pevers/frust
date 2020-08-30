#!/bin/bash
# gpio mode 29 up  # for the switch
# gpio -g mode 18 OUT  # for the SSR
# gpio -g mode 23 OUT  # for the SSR
/home/pi/.nvm/versions/node/v13.12.0/bin/node build/server.js

# sudo systemctl daemon-reload
# sudo systemctl restart fridge