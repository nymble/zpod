# Battery (M6)

Hardware: MAX17048 fuel gauge on I2C1 at **0x36** (confirmed). Registers used:
VCELL (0x02, 78.125 µV/LSB), SOC (0x04, 1/256 %), CRATE (0x16, signed,
0.208 %/h per LSB; positive = charging). The charger itself is not readable;
"charging" is inferred from CRATE.

## zpod-battery service

`/usr/lib/zpod/bin/zpod-battery` (Rust, `apps/crates/battery`), unit
`zpod-battery.service`. Replaces the M3-M5 Python `zpod-battery-guard`
(the Python file is still in the image but its unit is disabled).

- **Reads** the gauge every 30 s.
- **Logs** one line a minute to `/data/zpod/battery.log`:
  `<unix time> <SOC %> <VCELL V> <CRATE %/h> <charging 0|1>`.
  Only when the clock is sane (after 2025-01-01, i.e. the RTC or NTP set it)
  and `/data/zpod` exists. At 512 KiB (about 6 weeks) the file moves to
  `battery.log.1` (one old file kept), so at most ~1 MiB is used.
- **Sessions**: every unplug-to-plug discharge is written to
  `/data/zpod/battery-sessions.log` as `<start> <end> <SOC at start> <SOC at end>`
  (last 60 kept).
- **Low-battery guard** (unchanged rule): three readings in a row with SOC
  below `[battery] shutdown_pct` (default 5) **and** VCELL below
  `shutdown_mv` (default 3600) **and** CRATE ≤ 0 → `systemctl poweroff`.
  `shutdown_pct = 0` in `/etc/zpod/ui.conf` disables it.
- **No gauge** (not soldered, I2C dead): logs one line
  ("fuel gauge not answering") and retries every 60 s; nothing else breaks.
- `zpod-battery --once` prints one reading (or exits 1 without a gauge).

## Battery page (top menu > Battery)

| Field | How |
|---|---|
| Big % | SOC now (rounded) |
| State | **Charging** (CRATE > +0.5 %/h), **Charged** (SOC ≥ 99 % and not discharging faster than 0.6 %/h), else **On battery** |
| Time left | discharge rate = mean CRATE over the last 15 min blended with the least-squares SOC trend over the last 30 min (smooths the gauge's noisy CRATE); time = SOC ÷ rate. Shown only when under 14 days. When charging: time to full from the charge rate |
| Last charged | end of the last charge session (charger unplugged), "x h ago" |
| Full charge lasts | median over past discharges of (duration × 100 ÷ SOC used), counting only discharges that used ≥ 15 % and lasted ≥ 30 min. "learning..." until there is one |
| Cell | VCELL |
| Chart | SOC since the last unplug (or the last 24 h) |

Everything except the big % needs the log, so a fresh card shows "learning"
for the history fields until it has run on battery for a while. Time left is
an estimate; the MAX17048's own model drifts at low charge.

## Check on the device

```sh
systemctl status zpod-battery --no-pager
tail -3 /data/zpod/battery.log
cat /data/zpod/battery-sessions.log
/usr/lib/zpod/bin/zpod-battery --once
```
