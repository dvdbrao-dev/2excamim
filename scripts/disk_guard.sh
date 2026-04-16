#!/bin/bash

SNAPSHOTS="/root/2excamim/var/market-watch/snapshots.jsonl"
MAX_LINES=50000

lines=$(wc -l < "$SNAPSHOTS" 2>/dev/null || echo 0)
if [ "$lines" -gt "$MAX_LINES" ]; then
  tail -n 10000 "$SNAPSHOTS" > "$SNAPSHOTS.tmp"
  mv "$SNAPSHOTS.tmp" "$SNAPSHOTS"
  logger "EXCAMIM: snapshots.jsonl rotated from $lines to 10000 lines"
fi

USAGE=$(df / 2>/dev/null | tail -1 | awk '{print $5}' | tr -d '%')
if [ -n "$USAGE" ] && [ "$USAGE" -gt 80 ]; then
  logger "EXCAMIM ALERT: disk usage ${USAGE}%"
  echo "EXCAMIM ALERT: disk ${USAGE}% full" | wall
fi
