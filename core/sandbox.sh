#!/bin/sh
# Runs a command against a private PipeWire + WirePlumber + pipewire-pulse with no hardware,
# no D-Bus and no saved state, so tests never touch the user's audio, apps or running Mixpilot.
# Usage: sandbox.sh <command> [args]
B=${TMPDIR:-/tmp}/mixpilot-sandbox
mkdir -p "$B/run" "$B/state" "$B/conf/wireplumber/wireplumber.conf.d"
cat >"$B/conf/wireplumber/wireplumber.conf.d/sandbox.conf" <<'EOF'
wireplumber.profiles = {
  sandbox = { inherits = [ policy, mixin.systemwide-session, mixin.stateless ] }
}
EOF
export XDG_RUNTIME_DIR="$B/run" PIPEWIRE_RUNTIME_DIR="$B/run" XDG_STATE_HOME="$B/state"
export PULSE_SERVER="unix:$B/run/pulse/native" PULSE_RUNTIME_PATH="$B/run/pulse"
export DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent DBUS_SYSTEM_BUS_ADDRESS=unix:path=/nonexistent
S= W= P=
stop() {
  for p in $P $W $S; do kill "$p" 2>/dev/null; done
  wait 2>/dev/null
}
trap stop EXIT
trap 'exit 130' INT TERM

pipewire >"$B/pipewire.log" 2>&1 & S=$!
sleep 0.5
XDG_CONFIG_HOME="$B/conf" wireplumber -p sandbox >"$B/wireplumber.log" 2>&1 & W=$!
pipewire-pulse >"$B/pulse.log" 2>&1 & P=$!
i=0
until pactl info >/dev/null 2>&1 && pw-cli ls Metadata 2>/dev/null | grep -q 'metadata.name = "default"'; do
  i=$((i + 1)); [ $i -gt 50 ] && { echo "sandbox did not come up"; exit 1; }
  sleep 0.1
done
"$@"
