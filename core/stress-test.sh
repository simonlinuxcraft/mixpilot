#!/bin/sh
# Stability checks against the running PipeWire, audio goes to a temporary null sink only.
# Usage: stress-test.sh [soak seconds, default 120]
cd "$(dirname "$0")" || exit 1
# Never against the user's real audio: only inside ./sandbox.sh
case "$PIPEWIRE_RUNTIME_DIR" in *mixpilot-sandbox*) ;; *) echo "run me through ./sandbox.sh"; exit 1 ;; esac
export LC_ALL=C
SOAK=${1:-120}
W=${TMPDIR:-/tmp}/mixpilot-stress
mkdir -p "$W/cfg/mixpilot"
BIN=./target/release/mixpilot-core
FAILS=0
D= T= MOD=

# Stops the looping subshell and its current pw-cat child by PID, never by name pattern.
stop_loop() {
  [ -n "$T" ] || return
  kill -STOP "$T" 2>/dev/null; pkill -P "$T" 2>/dev/null; kill "$T" 2>/dev/null; kill -CONT "$T" 2>/dev/null
  T=
}
cleanup() {
  stop_loop
  [ -n "$D" ] && kill "$D" 2>/dev/null && wait "$D" 2>/dev/null
  [ -n "$MOD" ] && pactl unload-module "$MOD" 2>/dev/null
  T= D= MOD=
}
trap cleanup EXIT
trap 'exit 130' INT TERM
check() { if [ "$2" = ok ]; then echo "  ok    $1"; else echo "  FAIL  $1 ($2)"; FAILS=$((FAILS + 1)); fi; }
alive() { kill -0 "$D" 2>/dev/null && echo ok || echo "daemon died: $(tail -2 "$W/daemon.log")"; }
start() {
  XDG_CONFIG_HOME="$W/cfg" $BIN >"$W/daemon.log" 2>&1 &
  D=$!
  sleep 1.5
}

python3 -c "
import wave,math,struct
w=wave.open('$W/tone.wav','w'); w.setnchannels(2); w.setsampwidth(2); w.setframerate(48000)
w.writeframes(b''.join(struct.pack('<hh',v,v) for v in (int(8000*math.sin(2*math.pi*440*i/48000)) for i in range(48000*2))))"
cat >"$W/cfg/mixpilot/config.json" <<EOF
{ "output": "mixpilot_test_hw", "bass": 60, "rules": [ { "match": "mp_test", "channel": "game" } ] }
EOF
MOD=$(pactl load-module module-null-sink sink_name=mixpilot_test_hw) || exit 1
DEF_SINK=$(pactl get-default-sink)
start

echo "-- soak ${SOAK}s with a looping stream, plus a fader change every second"
( while :; do pw-cat -p --target mixpilot_test_hw -P '{ node.name=mp_test_loop }' "$W/tone.wav" >/dev/null 2>&1; done ) &
T=$!
sleep 2
rss0=$(awk '/VmRSS/{print $2}' /proc/$D/status)
cpu0=$(awk '{print $14+$15}' /proc/$D/stat)
i=0
while [ $i -lt "$SOAK" ]; do
  v=$(( (i * 37) % 101 ))
  printf '{ "output": "mixpilot_test_hw", "bass": %s, "game": { "volume": %s }, "chatmix": %s, "rules": [ { "match": "mp_test", "channel": "game" } ] }' $v $v $(( v * 2 - 100 )) >"$W/cfg/mixpilot/c.tmp"
  mv "$W/cfg/mixpilot/c.tmp" "$W/cfg/mixpilot/config.json"
  sleep 1; i=$((i + 1))
done
cpu1=$(awk '{print $14+$15}' /proc/$D/stat)
rss1=$(awk '/VmRSS/{print $2}' /proc/$D/status)
stop_loop
check "alive after soak" "$(alive)"
pct=$(python3 -c "print(round(($cpu1-$cpu0)/$(getconf CLK_TCK)/$SOAK*100,2))")
echo "        cpu ${pct}% of one core, rss ${rss0} kB -> ${rss1} kB"
check "cpu below 5%" "$(python3 -c "print('ok' if $pct < 5 else 'too high')")"
check "memory stable (< 2 MB growth)" "$( [ $((rss1 - rss0)) -lt 2048 ] && echo ok || echo "+$((rss1 - rss0)) kB")"
routed=$(grep -Fc 'sorted ["pw-cat", "Game"]' "$W/daemon.log")
check "every loop stream got routed ($routed)" "$( [ "$routed" -ge $((SOAK / 3)) ] && echo ok || echo "only $routed")"

echo "-- 40 short streams at once"
n=0
while [ $n -lt 40 ]; do
  timeout 0.5 pw-cat -p --target mixpilot_test_hw -P "{ node.name=mp_test_burst$n }" "$W/tone.wav" >/dev/null 2>&1 &
  n=$((n + 1))
done
sleep 2
check "alive after burst" "$(alive)"

echo "-- output device disappears and comes back"
pactl unload-module "$MOD"; MOD=
sleep 1
check "alive without output device" "$(alive)"
MOD=$(pactl load-module module-null-sink sink_name=mixpilot_test_hw)
sleep 1
check "alive after device returns" "$(alive)"

echo "-- garbage configs"
# valid ones carry empty rules so the built-in rules never touch the user's real apps
for c in '' 'null' '[]' '{"game":{"volume":"loud"}}' '{"game":{"volume":1e309}}' '{"output":"mixpilot_test_hw","rules":[],"chatmix":-99999,"bass":99999,"master":-5}' '{"output":"mixpilot_test_hw","rules":[{"match":"mp","channel":""}]}'; do
  printf '%s' "$c" >"$W/cfg/mixpilot/config.json"; sleep 0.7
done
check "alive after garbage configs" "$(alive)"

echo "-- kill -9"
kill -9 "$D"; wait "$D" 2>/dev/null; D=
sleep 1
left=$(pw-cli ls Node | grep -cE 'node.name = "mixpilot_(game|chat|media|aux|output)"')
check "nodes gone after kill -9" "$( [ "$left" = 0 ] && echo ok || echo "$left left")"
check "default sink unchanged" "$( [ "$(pactl get-default-sink)" = "$DEF_SINK" ] && echo ok || echo "now $(pactl get-default-sink)")"

echo
[ $FAILS = 0 ] && echo "ALL OK" || echo "$FAILS FAILED (log $W/daemon.log)"
exit $FAILS
