#!/bin/sh
# Live checks for the sound and microphone features. Audio only goes to temporary null sinks and a
# virtual test microphone; the real microphone and speakers are never used.
cd "$(dirname "$0")" || exit 1
# Never against the user's real audio: only inside ./sandbox.sh
case "$PIPEWIRE_RUNTIME_DIR" in *mixpilot-sandbox*) ;; *) echo "run me through ./sandbox.sh"; exit 1 ;; esac
export LC_ALL=C
W=${TMPDIR:-/tmp}/mixpilot-feat
mkdir -p "$W/cfg/mixpilot"
BIN=./target/release/mixpilot-core
STATE=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/mixpilot/state.json
FAILS=0
D= FEED= MEDIA= CHAT= MODS=

cleanup() {
  for p in $FEED $MEDIA $CHAT $D; do kill "$p" 2>/dev/null; done
  wait 2>/dev/null
  for m in $MODS; do pactl unload-module "$m" 2>/dev/null; done
  FEED= MEDIA= CHAT= D= MODS=
}
trap cleanup EXIT
trap 'exit 130' INT TERM
check() { if [ "$2" = ok ]; then echo "  ok    $1"; else echo "  FAIL  $1 ($2)"; FAILS=$((FAILS + 1)); fi; }
module() { m=$(pactl load-module "$@") && MODS="$m $MODS"; }
links_of() { pw-link -l | awk -v n="$1:" 'index($0, n) == 1 {cur = 1; next} /^[^ ]/ {cur = 0} cur && /\|->/ {sub(/.*\|-> /, ""); sub(/:.*/, ""); print}' | sort -u | tr '\n' ' '; }
alive() { kill -0 "$D" 2>/dev/null && echo ok || echo "daemon died: $(tail -2 "$W/daemon.log")"; }

# patch the config: each argument is a JSON object deep-merged into it
cfg() {
  python3 - "$W/cfg/mixpilot/config.json" "$@" <<'EOF'
import json, os, sys
path = sys.argv[1]
c = json.load(open(path)) if os.path.exists(path) else {}
def merge(a, b):
    for k, v in b.items():
        if isinstance(v, dict) and isinstance(a.get(k), dict): merge(a[k], v)
        else: a[k] = v
for p in sys.argv[2:]: merge(c, json.loads(p))
open(path + '.tmp', 'w').write(json.dumps(c)); os.rename(path + '.tmp', path)
EOF
  sleep 0.4
}

# RMS of a 1 s recording; $1 = node, $2 = extra pw-record args
rms() {
  : >"$W/cap.wav"
  timeout 1.5 pw-record $2 --target "$1" --rate 48000 --channels 1 --format s16 "$W/cap.wav" >/dev/null 2>&1
  python3 -c "
import wave,array,math
try:
    w=wave.open('$W/cap.wav'); a=array.array('h',w.readframes(w.getnframes())); a=a[len(a)//3:]
    print('%.4f'%(math.sqrt(sum(x*x for x in a)/len(a))/32768) if a else 'nan')
except Exception: print('nan')"
}
near() { python3 -c "import sys; a,b=float('$1'),float('$2'); sys.exit(0 if abs(a-b)<=max(0.01,b*0.15) else 1)" 2>/dev/null && echo ok || echo "got $1, want $2"; }
less() { python3 -c "import sys; sys.exit(0 if float('$1') < float('$2') else 1)" 2>/dev/null && echo ok || echo "got $1, want below $2"; }

# max of a level over 1 s of state snapshots; $1 = python expression on the state dict s
level() {
  python3 -c "
import json,time
m=0.0
for _ in range(20):
    try: s=json.load(open('$STATE')); m=max(m, float($1))
    except Exception: pass
    time.sleep(0.05)
print('%.4f'%m)"
}

# "rms peak" of a 1.5 s recording from the virtual microphone
stats() {
  : >"$W/cap.wav"
  timeout 2 pw-record --target mixpilot_mic --rate 48000 --channels 1 --format s16 "$W/cap.wav" >/dev/null 2>&1
  python3 -c "
import wave,array,math
w=wave.open('$W/cap.wav'); a=array.array('h',w.readframes(w.getnframes()))[24000:]
print('%.4f %.4f'%(math.sqrt(sum(x*x for x in a)/max(1,len(a)))/32768, max(map(abs,a or [0]))/32768))"
}
between() { python3 -c "import sys; sys.exit(0 if $2 <= float('$1') <= $3 else 1)" 2>/dev/null && echo ok || echo "got $1, want $2..$3"; }
feed() {
  [ -n "$FEED" ] && kill "$FEED" 2>/dev/null
  pw-cat -p --target mp_test_micbus -P '{ node.name=feed_mic }' "$W/$1" >/dev/null 2>&1 &
  FEED=$!
  sleep 0.5
}
gaps() {
  python3 -c "
import wave,array
w=wave.open('$1'); a=array.array('h',w.readframes(w.getnframes()))[12000:]
g=r=0
for x in a:
    if abs(x)<40:
        r+=1
        if r==4: g+=1
    else: r=0
print(g)"
}

python3 -c "
import wave,math,struct,random
def save(name, gen, secs=60):
    w=wave.open('$W/'+name,'w'); w.setnchannels(2); w.setsampwidth(2); w.setframerate(48000)
    w.writeframes(b''.join(struct.pack('<hh',v,v) for v in (gen(i) for i in range(48000*secs))))
save('tone.wav', lambda i: int(16384*math.sin(2*math.pi*440*i/48000)))
save('quiet.wav', lambda i: int(32768*0.03*math.sin(2*math.pi*440*i/48000)), 20)
save('loud.wav', lambda i: int(32767*0.9*math.sin(2*math.pi*440*i/48000)), 20)
save('fan.wav', lambda i: int(random.gauss(0, 180)), 20)
random.seed(1)
save('noise.wav', lambda i: int(random.gauss(0, 3000)), 20)"

DEF_SINK=$(pactl get-default-sink); DEF_SRC=$(pactl get-default-source)
module module-null-sink sink_name=mp_test_out
module module-null-sink sink_name=mp_test_micbus
module module-remap-source source_name=mp_test_mic master=mp_test_micbus.monitor
printf "{}" >"$W/cfg/mixpilot/config.json"
cfg '{"output":"mp_test_out","input":"mp_test_mic","ducking":true,
      "mic":{"gain":100,"mute":false,"noise":"off","agc":false,"gate":false,"voice":"natural","monitor":false},
      "rules":[{"match":"mp_test_chat","channel":"chat"},{"match":"mp_test_media","channel":"media"}]}'
XDG_CONFIG_HOME="$W/cfg" $BIN >"$W/daemon.log" 2>&1 &
D=$!
sleep 1.5

echo "-- microphone"
check "running" "$(alive)"
check "microphone closed while nobody listens" "$([ -z "$(pw-link -l | grep 'mixpilot_mic_in:')" ] && echo ok || echo 'mic is linked')"
pw-cat -p --target mp_test_micbus -P '{ node.name=feed_mic }' "$W/tone.wav" >/dev/null 2>&1 &
FEED=$!
sleep 0.5
v=$(rms mixpilot_mic)
check "processed voice arrives at Mixpilot Mikrofon" "$(near "$v" 0.354)"
check "microphone opened for the listener" "$(grep -Fq 'mic_active' "$W/daemon.log" && echo ok || echo 'no activation logged')"
check "test source used, not the real microphone" "$(pw-link -l | grep -q 'mp_test_mic:capture' || grep -Fq 'input ["mp_test_mic"]' "$W/daemon.log" && echo ok || echo 'input target not applied')"
cfg '{"mic":{"gain":50}}'
check "mic gain 50 % -> 0.044" "$(near "$(rms mixpilot_mic)" 0.0442)"
cfg '{"mic":{"gain":100,"mute":true}}'
check "mic muted -> silence" "$(near "$(rms mixpilot_mic)" 0)"
cfg '{"mic":{"mute":false}}'
kill $FEED; FEED=
pw-cat -p --target mp_test_micbus -P '{ node.name=feed_noise }' "$W/noise.wav" >/dev/null 2>&1 &
FEED=$!
sleep 0.5
off=$(rms mixpilot_mic)
cfg '{"mic":{"noise":"strong"}}'
sleep 1
strong=$(rms mixpilot_mic)
check "noise suppression removes white noise ($off -> $strong)" "$(less "$strong" "$(python3 -c "print($off*0.3)")")"
cfg '{"mic":{"noise":"off"}}'

echo "-- microphone quality"
cfg '{"mic":{"agc":true,"voice":"natural","gate":false}}'
feed quiet.wav
sleep 2
set -- $(stats)
check "auto level lifts a quiet voice to about -23 dBFS (rms $1)" "$(between "$1" 0.045 0.11)"
feed loud.wav
sleep 1.5
set -- $(stats)
check "auto level tames a loud voice (rms $1)" "$(between "$1" 0.045 0.11)"
check "no peak above -3 dBFS (peak $2)" "$(between "$2" 0 0.709)"
cfg '{"mic":{"voice":"broadcast"}}'
set -- $(stats)
check "broadcast stays below -3 dBFS too (peak $2)" "$(between "$2" 0 0.709)"
# broadband noise shows the presets best: radio cuts everything outside 300 Hz to 3.5 kHz
cfg '{"mic":{"agc":false,"voice":"natural","noise":"off"}}'
feed noise.wav
natural=$(stats | cut -d' ' -f1)
cfg '{"mic":{"voice":"radio"}}'
radio=$(stats | cut -d' ' -f1)
check "radio preset is clearly different ($natural -> $radio)" "$(less "$radio" "$(python3 -c "print($natural*0.7)")")"
# realistic fan noise around -45 dBFS: the gate has to make the pauses silent
cfg '{"mic":{"voice":"natural","noise":"off","gate":false}}'
feed fan.wav
open_gate=$(stats | cut -d' ' -f1)
cfg '{"mic":{"gate":true}}'
sleep 1
closed_gate=$(stats | cut -d' ' -f1)
check "gate silences the pauses ($open_gate -> $closed_gate)" "$(less "$closed_gate" "$(python3 -c "print(max($open_gate*0.01, 0.00005))")")"
cfg '{"mic":{"gate":false,"noise":"off","monitor":true}}'
feed tone.wav
for q in 256 1024; do
  pw-metadata -n settings 0 clock.force-quantum $q >/dev/null
  sleep 1
  : >"$W/g.wav"
  timeout 2 pw-record --target mixpilot_mic --rate 48000 --channels 1 --format s16 "$W/g.wav" >/dev/null 2>&1
  check "no dropouts in Mixpilot Mikrofon at quantum $q" "$(n=$(gaps "$W/g.wav"); [ "$n" = 0 ] && echo ok || echo "$n dropouts")"
  : >"$W/m.wav"
  timeout 2 pw-record -P '{ stream.capture.sink=true }' --target mp_test_out --rate 48000 --channels 1 --format s16 "$W/m.wav" >/dev/null 2>&1
  check "no dropouts when monitoring at quantum $q" "$(n=$(gaps "$W/m.wav"); [ "$n" = 0 ] && echo ok || echo "$n dropouts")"
done
pw-metadata -n settings 0 clock.force-quantum 0 >/dev/null
cfg '{"mic":{"monitor":false}}'
kill $FEED; FEED=
sleep 3
check "microphone closes again after the listener left" "$(grep -Fq 'mic_paused' "$W/daemon.log" && echo ok || echo 'still open')"

echo "-- mixer"
pw-cat -p --target mp_test_out -P '{ node.name=mp_test_media application.name=mp_test_media }' "$W/tone.wav" >/dev/null 2>&1 &
MEDIA=$!
sleep 1
alone=$(level "s['levels']['media'][0]")
pw-cat -p --target mp_test_out -P '{ node.name=mp_test_chat application.name=mp_test_chat }' "$W/tone.wav" >/dev/null 2>&1 &
CHAT=$!
sleep 1.5
ducked=$(level "s['levels']['media'][0]")
check "ducking: Media steps back while Chat talks ($alone -> $ducked)" "$(near "$ducked" "$(python3 -c "print($alone*0.25)")")"
cfg '{"ducking":false}'
sleep 1
check "ducking off restores Media" "$(near "$(level "s['levels']['media'][0]")" "$alone")"
kill $CHAT; CHAT=
sleep 0.5
before=$(rms mp_test_out "-P {stream.capture.sink=true}")
cfg '{"eq":[0,0,0,0,-12,0,0,0,0,0]}'
after=$(rms mp_test_out "-P {stream.capture.sink=true}")
check "equalizer -12 dB at 500 Hz lowers a 440 Hz tone ($before -> $after)" "$(less "$after" "$(python3 -c "print($before*0.5)")")"
cfg '{"eq":[0,0,0,0,0,0,0,0,0,0]}'

echo "-- automatic sorting"
check "media tone sits in Media" "$([ "$(links_of mp_test_media)" = "mixpilot_media " ] && echo ok || echo "linked to: $(links_of mp_test_media)")"
cfg '{"rules":[{"match":"mp_test_media","channel":"aux","user":true},{"match":"mp_test_chat","channel":"chat"}]}'
sleep 0.5
check "user rule moves the running app to Aux" "$([ "$(links_of mp_test_media)" = "mixpilot_aux " ] && echo ok || echo "linked to: $(links_of mp_test_media)")"
check "state file lists the app in Aux" "$(python3 -c "
import json; s=json.load(open('$STATE'))
print('ok' if any(a['name']=='mp_test_media' and a['channel']=='aux' for a in s['apps']) else s['apps'])")"
check "state file has events" "$(python3 -c "import json; s=json.load(open('$STATE')); print('ok' if s['events'] else 'none')")"

echo "-- shutdown"
kill $MEDIA; MEDIA=
kill -TERM $D; wait $D; rc=$?; D=
check "clean exit" "$([ $rc = 0 ] && echo ok || echo "rc=$rc")"
sleep 0.5
check "no mixpilot nodes left" "$([ "$(pw-cli ls Node | grep -c 'node.name = "mixpilot_')" = 0 ] && echo ok || echo left)"
check "state file removed" "$([ ! -e "$STATE" ] && echo ok || echo exists)"
echo
[ $FAILS = 0 ] && echo "ALL OK" || echo "$FAILS FAILED (log $W/daemon.log)"
exit $FAILS
