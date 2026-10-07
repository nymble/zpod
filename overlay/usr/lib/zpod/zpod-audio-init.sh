#!/bin/sh
# First boot only: default mixer for the Aoide ZPOD DAC (PCM5122).
# Digital ~80 %, analogue boost on. Afterwards alsa-restore/alsa-state keep
# whatever the user sets.
CARD=sndrpiaoidezpod
i=0
while ! grep -q "\[$CARD" /proc/asound/cards 2>/dev/null; do
  i=$((i + 1))
  if [ "$i" -ge 30 ]; then
    echo "zpod-audio-init: card $CARD not present after 30 s; leaving defaults" >&2
    exit 0
  fi
  sleep 1
done
amixer -q -c "$CARD" sset 'Digital' 80% || true
amixer -q -c "$CARD" sset 'Digital' unmute 2>/dev/null || true
amixer -q -c "$CARD" sset 'Analogue Playback Boost' 100% || true
alsactl store "$CARD" 2>/dev/null || alsactl store || true
mkdir -p /var/lib/zpod
touch /var/lib/zpod/audio-defaults-applied
echo "zpod-audio-init: $CARD Digital=80% boost=on stored"
