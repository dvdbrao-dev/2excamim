#!/bin/bash

if ! python3 -c "import socket; socket.gethostbyname('api.openai.com')" 2>/dev/null; then
  echo "nameserver 8.8.8.8" > /etc/resolv.conf
  echo "nameserver 1.1.1.1" >> /etc/resolv.conf
  netplan apply 2>/dev/null || true
  systemctl restart systemd-resolved 2>/dev/null || true
  logger "EXCAMIM: DNS repaired by watchdog"
fi
