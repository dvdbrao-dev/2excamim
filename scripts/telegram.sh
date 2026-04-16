#!/bin/bash
source /root/2excamim/.env
MSG="$1"
curl -s -X POST "https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}/sendMessage" \
  -d chat_id="${TELEGRAM_CHAT_ID}" \
  -d text="${MSG}" \
  -d parse_mode="Markdown" \
  > /dev/null
