#!/bin/bash

STORE="/root/2excamim/var/events.jsonl"

FAILURES=$(
  journalctl --no-pager -u market-watch.service --since "24 hours ago" 2>/dev/null | \
    grep "skipped_api_failure" | \
    python3 -c "
import sys
import re

total = 0
for line in sys.stdin:
    match = re.search(r'skipped_api_failure.:(\\d+)', line)
    if match:
        total += int(match.group(1))
print(total)
"
)

if [ -z "$FAILURES" ]; then
  FAILURES=0
fi

if [ "$FAILURES" -gt 50 ]; then
  logger "EXCAMIM ALERT: OpenAI API failures=$FAILURES in last 24h - check credits"
  echo "EXCAMIM ALERT: OpenAI API failures=$FAILURES" | wall
fi
