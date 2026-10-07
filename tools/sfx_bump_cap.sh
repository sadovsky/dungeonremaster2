#!/bin/bash
# sfx_bump.sh CONF OUT : start a new game in DOSBox with CONF, walk north up the
# start corridor and bump the end wall 4 times (the champion cries out), while
# recording the WSLg PulseAudio monitor to OUT.wav. Bump times (seconds from
# the start of the recording) go to OUT.events. Used to check that the
# original's digital sound effects reach the recording (docs/11).
CONF=$1; OUT=$2
dosbox -conf "$CONF" > "$OUT.dosbox.log" 2>&1 &
PID=$!
W=""
for i in $(seq 1 40); do
  W=$(xdotool search --pid $PID 2>/dev/null | head -1)
  [ -n "$W" ] && break
  sleep 0.5
done
sleep 9
xdotool key --window "$W" Escape; sleep 4
xdotool key --window "$W" Escape; sleep 5
xdotool windowactivate --sync "$W" mousemove --window "$W" 110 64 click 1; sleep 6
xdotool mousemove --window "$W" 300 195
xdotool key --window "$W" ctrl+F6
ffmpeg -v error -y -f pulse -i RDPSink.monitor -t 22 "$OUT.wav" &
FF=$!
T0=$(date +%s.%N)
: > "$OUT.events"
sleep 2
for i in 1 2 3 4 5 6 7 8; do xdotool key --window "$W" Up; sleep 0.9; done
for i in 1 2 3 4; do
  echo "bump $(echo "$(date +%s.%N) - $T0" | bc)" >> "$OUT.events"
  xdotool key --window "$W" Up; sleep 2
done
xdotool key --window "$W" ctrl+F6
wait $FF
kill $PID 2>/dev/null
wait $PID 2>/dev/null
