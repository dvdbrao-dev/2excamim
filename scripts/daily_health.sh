#!/bin/bash

cd /root/2excamim || exit 1

echo "=== EXCAMIM Daily Health $(date) ==="
systemctl is-active market-watch.timer 2>/dev/null || true
python3 scripts/pnl_summary.py
echo "Disk: $(df -h / | tail -1 | awk '{print $5}')"
echo "Snapshots: $(wc -l < var/market-watch/snapshots.jsonl 2>/dev/null || echo 0) lines"
echo "Events: $(wc -l < var/events.jsonl 2>/dev/null || echo 0) lines"
python3 agents/telegram_agent.py \
  --store ./var/events.jsonl \
  --daily-summary 2>&1
