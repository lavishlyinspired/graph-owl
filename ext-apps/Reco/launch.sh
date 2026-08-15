#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────
#  launch.sh  —  Start Reco (Matcha) locally
#  Backend  : http://localhost:8000   (FastAPI / uvicorn)
#  Frontend : http://localhost:5173   (Vite / React)
# ─────────────────────────────────────────────────────────────────

set -e

ROOT="$(cd "$(dirname "$0")" && pwd)"
BACKEND="$ROOT/backend"
FRONTEND="$ROOT/frontend"
VENV="$BACKEND/.venv"

GREEN='\033[0;32m'
AMBER='\033[0;33m'
BLUE='\033[0;34m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${GREEN}"
echo "  ██████╗ ███████╗ ██████╗ ██████╗ "
echo "  ██╔══██╗██╔════╝██╔════╝██╔═══██╗"
echo "  ██████╔╝█████╗  ██║     ██║   ██║"
echo "  ██╔══██╗██╔══╝  ██║     ██║   ██║"
echo "  ██║  ██║███████╗╚██████╗╚██████╔╝"
echo "  ╚═╝  ╚═╝╚══════╝ ╚═════╝ ╚═════╝ "
echo -e "${NC}"
echo -e "${BLUE}  GST Reconciliation — Local Dev Launcher${NC}"
echo "  ─────────────────────────────────────────"

# 1. Python virtual env
if [ ! -d "$VENV" ]; then
  echo -e "${AMBER}  → Creating Python virtual environment...${NC}"
  python3 -m venv "$VENV"
fi

echo -e "${AMBER}  → Installing backend deps...${NC}"
source "$VENV/bin/activate"
pip install -r "$BACKEND/requirements.txt" -q --disable-pip-version-check

# 2. Node deps
if [ ! -d "$FRONTEND/node_modules" ]; then
  echo -e "${AMBER}  → Installing frontend dependencies...${NC}"
  cd "$FRONTEND" && npm install --silent
fi

# 3. Kill old servers on those ports
kill_port() {
  local port=$1
  local pid
  pid=$(lsof -ti tcp:"$port" 2>/dev/null) && kill "$pid" 2>/dev/null && echo -e "${AMBER}  → Freed port $port${NC}" || true
}
kill_port 8000
kill_port 5173

# 4. Start backend
echo -e "${GREEN}  → Starting backend on http://localhost:8000 ...${NC}"
cd "$BACKEND"
uvicorn app.main:app --host 127.0.0.1 --port 8000 --reload > /tmp/reco-backend.log 2>&1 &
BACKEND_PID=$!

# 5. Wait for backend
echo -n "  → Waiting for backend"
for i in $(seq 1 20); do
  if curl -s http://127.0.0.1:8000/api/health > /dev/null 2>&1; then
    echo -e " ${GREEN}✓${NC}"
    break
  fi
  echo -n "."
  sleep 0.5
done

# 6. Start frontend
echo -e "${GREEN}  → Starting frontend on http://localhost:5173 ...${NC}"
cd "$FRONTEND"
npm run dev -- --port 5173 > /tmp/reco-frontend.log 2>&1 &
FRONTEND_PID=$!

# 7. Open browser
sleep 2
echo -e "${BLUE}  → Opening http://localhost:5173 in your browser...${NC}"
open "http://localhost:5173" 2>/dev/null || xdg-open "http://localhost:5173" 2>/dev/null || true

echo ""
echo -e "${GREEN}  ✓ Reco is running!${NC}"
echo "  ─────────────────────────────────────────"
echo -e "  Frontend : ${BLUE}http://localhost:5173${NC}"
echo -e "  Backend  : ${BLUE}http://localhost:8000${NC}"
echo -e "  API Docs : ${BLUE}http://localhost:8000/docs${NC}"
echo ""
echo -e "  Logs:"
echo "    Backend  → /tmp/reco-backend.log"
echo "    Frontend → /tmp/reco-frontend.log"
echo ""
echo -e "${AMBER}  Press Ctrl+C to stop all servers.${NC}"
echo ""

# 8. Cleanup on Ctrl+C
cleanup() {
  echo ""
  echo -e "${RED}  → Shutting down...${NC}"
  kill "$BACKEND_PID" 2>/dev/null || true
  kill "$FRONTEND_PID" 2>/dev/null || true
  kill_port 8000
  kill_port 5173
  echo -e "${GREEN}  ✓ Stopped. Goodbye!${NC}"
  exit 0
}
trap cleanup INT TERM

# 9. Keep alive
wait
