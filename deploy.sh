#!/bin/bash
set -e

REPO_DIR="/home/d88u525/Projects/rust/swarm-dispatcher"
LOG_FILE="/home/d88u525/Projects/rust/swarm-dispatcher/deploy.log"

echo "[$(date '+%Y-%m-%d %H:%M:%S')] === 🚀 Auto-deploy triggered ===" | tee -a "$LOG_FILE"

cd "$REPO_DIR"

# Проверка изменений
BEFORE=$(git rev-parse HEAD)
git fetch origin master 2>&1 | tee -a "$LOG_FILE"
AFTER=$(git rev-parse origin/master)

if [ "$BEFORE" = "$AFTER" ]; then
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] ✅ No changes, skipping" | tee -a "$LOG_FILE"
    exit 0
fi

echo "[$(date '+%Y-%m-%d %H:%M:%S')] 📥 Pulling changes: $BEFORE -> $AFTER" | tee -a "$LOG_FILE"
git pull origin master 2>&1 | tee -a "$LOG_FILE"

echo "[$(date '+%Y-%m-%d %H:%M:%S')] 🔨 Building release binary..." | tee -a "$LOG_FILE"
cargo build --release 2>&1 | tee -a "$LOG_FILE"

echo "[$(date '+%Y-%m-%d %H:%M:%S')] 🐳 Restarting containers..." | tee -a "$LOG_FILE"
docker compose up -d --build 2>&1 | tee -a "$LOG_FILE"

echo "[$(date '+%Y-%m-%d %H:%M:%S')] ✅ Deploy completed" | tee -a "$LOG_FILE"
