#!/bin/sh
# What users keep after Mixpilot is quit or uninstalled: the system default microphone must fall back
# to a real one, and a sorted app must play on the real output again.
cd "$(dirname "$0")" || exit 1
case "$PIPEWIRE_RUNTIME_DIR" in *mixpilot-sandbox*) ;; *) echo "run me through ./sandbox.sh"; exit 1 ;; esac
export LC_ALL=C
W=${TMPDIR:-/tmp}/mixpilot-uninstall
mkdir -p "$W/cfg/mixpilot"
FAILS=0
D= T= M1= M2=
cleanup() {
  [ -n "$T" ] && kill "$T" 2>/dev/null
  [ -n "$D" ] && kill "$D" 2>/dev/null && wait "$D" 2>/dev/null
  [ -n "$M1" ] && pactl unload-module "$M1" 2>/dev/null
  [ -n "$M2" ] && pactl unload-module "$M2" 2>/dev/null
}
trap cleanup EXIT
trap 'exit 130' INT TERM
check() { if [ "$2" = ok ]; then echo "  ok    $1"; else echo "  FAIL  $1 ($2)"; FAILS=$((FAILS + 1)); fi; }
wait_for() { i=0; until eval "$1"; do i=$((i + 1)); [ $i -gt 50 ] && return 1; sleep 0.1; done; }
node_exists() { pw-cli ls Node | grep -q "node.name = \"$1\""; }

M1=$(pactl load-module module-null-sink sink_name=test_hw)
M2=$(pactl load-module module-null-sink sink_name=test_mic media.class=Audio/Source)
pactl set-default-sink test_hw
pactl set-default-source test_mic
echo '{"setup_done":true,"output":"test_hw","input":"test_mic"}' >"$W/cfg/mixpilot/config.json"

XDG_CONFIG_HOME="$W/cfg" ./target/release/mixpilot-core >"$W/core.log" 2>&1 & D=$!
wait_for 'node_exists mixpilot_mic' || { echo "core did not come up"; cat "$W/core.log"; exit 1; }

# the app's "Als Standard-Mikrofon setzen"
pw-metadata -n default 0 default.configured.audio.source '{"name":"mixpilot_mic"}' Spa:String:JSON >/dev/null
wait_for '[ "$(pactl get-default-source)" = mixpilot_mic ]'
check "Mixpilot Mikrofon is the default" "$([ "$(pactl get-default-source)" = mixpilot_mic ] && echo ok || pactl get-default-source)"

# an app that gets sorted into a channel (rule "spotify" -> media)
pw-play --properties '{ application.name = "spotify" application.process.binary = "spotify" }' --volume 0.1 /usr/share/sounds/alsa/Front_Center.wav >/dev/null 2>&1 &
T=$!
sleep 1
pw-link -l | grep -q "mixpilot_media" && check "app sorted into Media" ok || check "app sorted into Media" "not linked"

kill "$D"; wait "$D" 2>/dev/null; D=
sleep 1
check "no Mixpilot nodes left" "$(pw-cli ls Node | grep -q 'node.name = "mixpilot' && echo left || echo ok)"
# WirePlumber then picks the best available microphone, like after unplugging a device. The sandbox has no
# real microphones, so only "no longer Mixpilot" can be checked here.
check "default microphone is no longer Mixpilot" "$(pactl get-default-source | grep -q '^mixpilot' && pactl get-default-source || echo ok)"
kill "$T" 2>/dev/null; wait "$T" 2>/dev/null; T=

# a new stream of the same app after the core is gone must reach the real output
pw-play --properties '{ application.name = "spotify" application.process.binary = "spotify" }' --volume 0.1 /usr/share/sounds/alsa/Front_Center.wav >/dev/null 2>&1 &
T=$!
sleep 1
check "app plays on the real output again" "$(pw-link -l | grep -A3 '^spotify\|pw-play' | grep -q 'test_hw' && echo ok || echo 'not on test_hw')"
echo "configured default left behind: $(pw-metadata -n default 0 default.configured.audio.source 2>/dev/null | grep -o 'value:.*' | head -1)"
[ $FAILS -eq 0 ] && echo "uninstall-test: all ok" || echo "uninstall-test: $FAILS failed"
exit $FAILS
