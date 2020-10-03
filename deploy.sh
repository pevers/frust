#!/bin/bash
sudo systemctl stop frust
sudo systemctl start frust
sudo systemctl status frust.service

sudo systemctl stop frust-server
sudo systemctl start frust-server
sudo systemctl status frust.service