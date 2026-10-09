#!/bin/sh
# Live test against the running PipeWire. Plays into a temporary null sink only, never to real speakers.
# Leaves nothing behind: null sink, daemon and tone are removed on exit, also on Ctrl+C.
cd "$(dirname "$0")" || exit 1
# Never against the user's real audio: only inside ./sandbox.sh
case "$PIPEWIRE_RUNTIME_DIR" in *mixpilot-sandbox*) ;; *) echo "run me through ./sandbox.sh"; exit 1 ;; esac
export LC_ALL=C
W=${TMPDIR:-/tmp}/mixpilot-live
mkdir -p "$W/cfg/mixpilot"
BIN=./target/release/mixpilot-core
FAILS=0
D= T= MOD= MOD2=
OUT=mixpilot_test_hw

cleanup() {
  [ -n "$T" ] && kill "$T" 2>/dev/null
  [ -n "$D" ] && kill "$D" 2>/dev/null && wait "$D" 2>/dev/null
  [ -n "$MOD" ] && pactl unload-module "$MOD" 2>/dev/null
  [ -n "$MOD2" ] && pactl unload-module "$MOD2" 2>/dev/null
  T= D= MOD= MOD2=
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# Devices the given node's output ports are linked to, parsed per port block.
links_of() { pw-link -l | awk -v n="$1:" 'index($0, n) == 1 {cur = 1; next} /^[^ ]/ {cur = 0} cur && /\|->/ {sub(/.*\|-> /, ""); sub(/:.*/, ""); print}' | sort -u | tr '\n' ' '; }
outlinks() { links_of mixpilot_output; }
check() { if [ "$2" = ok ]; then echo "  ok    $1"; else echo "  FAIL  $1 ($2)"; FAILS=$((FAILS + 1)); fi; }

# RMS of a 1 s capture from the null sink's monitor
rms() {
  : >"$W/cap.wav"
  timeout 1.5 pw-record -P '{ stream.capture.sink=true }' --target mixpilot_test_hw --rate 48000 --channels 2 --format s16 "$W/cap.wav" >/dev/null 2>&1
  python3 -c "
import wave,array,math,sys
try:
    w=wave.open('$W/cap.wav'); a=array.array('h',w.readframes(w.getnframes()))
    a=a[len(a)//4:]
    print('%.4f'%(math.sqrt(sum(x*x for x in a)/len(a))/32768) if a else 'nan')
except Exception: print('nan')"
}

near() { python3 -c "import sys; a,b=float('$1'),float('$2'); sys.exit(0 if abs(a-b)<=max(0.01,b*0.15) else 1)" 2>/dev/null && echo ok || echo "got $1, want $2"; }

setcfg() {
  cat >"$W/cfg/mixpilot/config.json.tmp" <<EOF
{ "output": "$OUT", "chat": { "volume": $1, "mute": $2 }, "chatmix": $3, "bass": 0, "limiter": true,
  "rules": [ { "match": "mp_test_tone", "channel": "chat" }, { "match": "mp_test_nan", "channel": "chat" } ] }
EOF
  mv "$W/cfg/mixpilot/config.json.tmp" "$W/cfg/mixpilot/config.json"
  sleep 0.6
}

python3 -c "
import wave,math,struct
w=wave.open('$W/tone.wav','w'); w.setnchannels(2); w.setsampwidth(2); w.setframerate(48000)
w.writeframes(b''.join(struct.pack('<hh',v,v) for v in (int(16384*math.sin(2*math.pi*440*i/48000)) for i in range(48000*60))))"

DEF_SINK=$(pactl get-default-sink)
MOD=$(pactl load-module module-null-sink sink_name=mixpilot_test_hw) || { echo "cannot create test sink"; exit 1; }
setcfg 100 false 0

XDG_CONFIG_HOME="$W/cfg" $BIN >"$W/daemon.log" 2>&1 &
D=$!
sleep 1.5
echo "-- daemon"
kill -0 $D 2>/dev/null && check "running" ok || check "running" "exited: $(cat "$W/daemon.log")"
n=$(pw-cli ls Node | grep -cE 'node.name = "mixpilot_(game|chat|media|music|aux)"')
check "5 channel sinks exist" "$([ "$n" = 5 ] && echo ok || echo "found $n")"
check "output linked only to test sink" "$([ "$(outlinks)" = "mixpilot_test_hw " ] && echo ok || echo "linked to: $(outlinks)")"

pw-cat -p --target mixpilot_test_hw -P '{ node.name=mp_test_tone application.name=mp_test_tone }' "$W/tone.wav" >/dev/null 2>&1 &
T=$!
sleep 1.5
echo "-- routing"
grep -Fq 'sorted ["mp_test_tone", "Chat"]' "$W/daemon.log" && check "rule matched" ok || check "rule matched" "log: $(tail -3 "$W/daemon.log")"
tl=$(links_of mp_test_tone)
check "tone moved into mixpilot_chat" "$([ "$tl" = "mixpilot_chat " ] && echo ok || echo "linked to: $tl")"

echo "-- levels (sine peak 0.5, RMS 0.354)"
check "chat 100       -> 0.354" "$(near "$(rms)" 0.354)"
setcfg 50 false 0
check "chat 50 (cubic) -> 0.044" "$(near "$(rms)" 0.0442)"
setcfg 100 true 0
check "chat muted     -> 0" "$(near "$(rms)" 0)"
setcfg 100 false -50
check "chatmix -50    -> 0.177" "$(near "$(rms)" 0.177)"
printf '{ broken json' >"$W/cfg/mixpilot/config.json"; sleep 1.2
check "broken config keeps last values" "$(near "$(rms)" 0.177)"
kill -0 $D 2>/dev/null && check "daemon survived broken config" ok || check "daemon survived broken config" exited
setcfg 100 false 0

echo "-- output device switching"
MOD2=$(pactl load-module module-null-sink sink_name=mixpilot_test_hw2)
OUT=mixpilot_test_hw2; setcfg 100 false 0; sleep 0.5
check "switch to second device" "$([ "$(outlinks)" = "mixpilot_test_hw2 " ] && echo ok || echo "linked to: $(outlinks)")"
OUT=; setcfg 100 true 0; sleep 0.5
check "empty output follows system default (muted, silent)" "$([ "$(outlinks)" = "$(pactl get-default-sink) " ] && echo ok || echo "linked to: $(outlinks), default: $(pactl get-default-sink)")"
OUT=mixpilot_test_hw; setcfg 100 false 0; sleep 0.5
check "switch back" "$([ "$(outlinks)" = "mixpilot_test_hw " ] && echo ok || echo "linked to: $(outlinks)")"
check "audio arrives after switching back" "$(near "$(rms)" 0.354)"
pactl unload-module "$MOD2"; MOD2=

echo "-- robustness"
XDG_CONFIG_HOME="$W/cfg" timeout 5 $BIN >"$W/second.log" 2>&1
check "second instance refused (exit 3)" "$([ $? = 3 ] && echo ok || echo "rc=$?: $(cat "$W/second.log")")"
kill $T; T=
python3 -c "
import struct,math
n=float('nan'); i=float('inf')
bad=b''.join(struct.pack('<ff',v,-v) for v in [n,i,-i,1e30]*1200)
good=b''.join(struct.pack('<ff',v,v) for v in (0.5*math.sin(2*math.pi*440*k/48000) for k in range(48000*20)))
open('$W/nan.raw','wb').write(bad+good)"
pw-cat -p --raw --format f32 --rate 48000 --channels 2 --target mixpilot_test_hw -P '{ node.name=mp_test_nan application.name=mp_test_nan }' "$W/nan.raw" >/dev/null 2>&1 &
T=$!
sleep 2
check "audio recovers after NaN/inf from a client" "$(near "$(rms)" 0.354)"
kill -0 $D 2>/dev/null && check "daemon survived NaN/inf" ok || check "daemon survived NaN/inf" exited

echo "-- shutdown"
kill $T; T=
kill -TERM $D; wait $D; rc=$?; D=
check "clean exit on SIGTERM" "$([ $rc = 0 ] && grep -q stopped "$W/daemon.log" && echo ok || echo "rc=$rc")"
sleep 0.5
left=$(pw-cli ls Node | grep -c 'node.name = "mixpilot_')
check "no mixpilot nodes left" "$([ "$left" = 1 ] && echo ok || echo "$left left (1 = test sink)")"
echo
[ $FAILS = 0 ] && echo "ALL OK" || echo "$FAILS FAILED (daemon log: $W/daemon.log)"
exit $FAILS
