#!/bin/sh
# Simulates a WirePlumber restart and checks that every channel route is written again.
# Runs on a private PipeWire with an isolated WirePlumber: no device monitors, no D-Bus, no saved
# state. The user's own PipeWire, WirePlumber and their stored routes are never touched.
cd "$(dirname "$0")" || exit 1
B=${TMPDIR:-/tmp}/mixpilot-wptest
mkdir -p "$B/run" "$B/state" "$B/conf/wireplumber/wireplumber.conf.d" "$B/conf/mixpilot"
export LC_ALL=C PIPEWIRE_RUNTIME_DIR="$B/run" XDG_STATE_HOME="$B/state" XDG_CONFIG_HOME="$B/conf"
export DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent DBUS_SYSTEM_BUS_ADDRESS=unix:path=/nonexistent
cat >"$B/conf/wireplumber/wireplumber.conf.d/mptest.conf" <<'EOF'
wireplumber.profiles = {
  mptest = { inherits = [ policy, mixin.systemwide-session, mixin.stateless ] }
}
EOF
echo '{ "rules": [ { "match": "mp_wp", "channel": "chat" } ] }' >"$B/conf/mixpilot/config.json"
FAILS=0
S= W= D= T1= T2=
cleanup() {
  for p in $T1 $T2 $D $W $S; do kill "$p" 2>/dev/null; done
  wait 2>/dev/null
}
trap cleanup EXIT
trap 'exit 130' INT TERM
check() { if [ "$2" = ok ]; then echo "  ok    $1"; else echo "  FAIL  $1 ($2)"; FAILS=$((FAILS + 1)); fi; }
routes() { timeout 1 pw-metadata -n default 2>/dev/null | grep -c "target.object.*mixpilot_chat"; }
wait_md() { i=0; while [ $i -lt 30 ] && ! pw-cli ls Metadata 2>/dev/null | grep -q 'metadata.name = "default"'; do sleep 0.2; i=$((i + 1)); done; }
tone=${TMPDIR:-/tmp}/mixpilot-live/tone.wav
[ -f "$tone" ] || { echo "run live-test.sh first (creates $tone)"; exit 1; }

pipewire >"$B/pw.log" 2>&1 & S=$!
sleep 1
wireplumber -p mptest >"$B/wp1.log" 2>&1 & W=$!
wait_md
XDG_RUNTIME_DIR="$B/run" ./target/release/mixpilot-core >"$B/daemon.log" 2>&1 & D=$!
sleep 1
pw-cat -p -P '{ node.name=mp_wp_one application.name=mp_wp_one }' "$tone" >/dev/null 2>&1 & T1=$!
sleep 1.5
check "route written while WirePlumber runs" "$([ "$(routes)" = 1 ] && echo ok || echo "routes: $(routes)")"

kill "$W"; wait "$W" 2>/dev/null; W=
sleep 0.5
check "daemon survives WirePlumber exit" "$(kill -0 $D 2>/dev/null && echo ok || echo exited)"
pw-cat -p -P '{ node.name=mp_wp_two application.name=mp_wp_two }' "$tone" >/dev/null 2>&1 & T2=$!
sleep 1

wireplumber -p mptest >"$B/wp2.log" 2>&1 & W=$!
wait_md
sleep 1.5
check "both routes back after restart (old + gap)" "$([ "$(routes)" = 2 ] && echo ok || echo "routes: $(routes)")"
links=$(pw-link -l 2>/dev/null | grep -A2 '^mp_wp_' | grep -c -- '|-> mixpilot_chat')
check "both streams linked into mixpilot_chat" "$([ "$links" -ge 4 ] && echo ok || echo "$links port links")"

pw-cli create-node adapter '{ factory.name=support.null-audio-sink node.name=mp_test_late media.class=Audio/Sink object.linger=true audio.position=[FL FR] }' >/dev/null 2>&1
sleep 3
out=$(pw-link -l 2>/dev/null | grep -A2 '^mixpilot_output' | grep -c -- '|-> mp_test_late')
check "output finds a device that appears later" "$([ "$out" -ge 2 ] && echo ok || echo "$out port links")"
flaps=$(grep -c "output connected again" "$B/daemon.log")
check "no log flapping while there was no device" "$([ "$flaps" -le 1 ] && echo ok || echo "$flaps reconnect lines")"
check "daemon still running" "$(kill -0 $D 2>/dev/null && echo ok || echo exited)"

echo
[ $FAILS = 0 ] && echo "ALL OK" || { echo "$FAILS FAILED"; cat "$B/daemon.log"; }
exit $FAILS
