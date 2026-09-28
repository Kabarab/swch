#!/bin/bash
# Start the real SWCH API and open its UI in a desktop-style Chrome window.
set -e
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$DIR"
SWCH_URL="http://127.0.0.1:4242"
SWCH_LOG="${TMPDIR:-/tmp}/swch-startup.log"
export SWCH_KEYRING_BACKEND="${SWCH_KEYRING_BACKEND:-memory}"
health_ready() {
    curl --noproxy '*' --silent --fail --max-time 2 "$SWCH_URL/api/health" | grep -Eq '"status"[[:space:]]*:[[:space:]]*"ok"'
}
if ! health_ready; then
    if nc -z 127.0.0.1 4242 2>/dev/null; then
        echo "Порт 4242 занят другим сервисом (возможно, python http.server)."
        echo "Остановите его и запустите SWCH ещё раз. Чужой процесс не остановлен."
        exit 1
    fi
    BINARY=""
    if [[ -x "$DIR/target_build/debug/swch" ]]; then
        BINARY="$DIR/target_build/debug/swch"
    elif [[ -x "$HOME/swch2/target_build/debug/swch" ]]; then
        BINARY="$HOME/swch2/target_build/debug/swch"
    fi
    if [[ -z "$BINARY" ]]; then
        if ! command -v cargo >/dev/null 2>&1; then
            export PATH="$HOME/.cargo/bin:$PATH"
        fi
        echo "Сборка SWCH…"
        cargo build
        SWCH_TARGET_DIR="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
        BINARY="$SWCH_TARGET_DIR/debug/swch"
    fi
    "$BINARY" > "$SWCH_LOG" 2>&1 &
    SWCH_PID=$!
    for ((i=0; i<60; i++)); do
        if health_ready; then break; fi
        if ! kill -0 "$SWCH_PID" 2>/dev/null; then
            echo "SWCH не запустился. Журнал: $SWCH_LOG"
            exit 1
        fi
        sleep 1
    done
    if ! health_ready; then
        echo "Сервис ещё не готов. Журнал: $SWCH_LOG"
        exit 1
    fi
fi
SWCH_CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
if [[ -x "$SWCH_CHROME" ]]; then
    "$SWCH_CHROME" --app="$SWCH_URL" \
        --user-data-dir="$HOME/Library/Application Support/swch/desktop_profile" \
        --window-size=1440,960 > /dev/null 2>&1 &
else
    open "$SWCH_URL"
fi
