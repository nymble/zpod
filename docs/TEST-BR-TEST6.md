# Test6: first in-place update (keeps /data)

Release `v0.11.0-br-ab-test6` (pre-release). Two parts: **A/B update of the
test5 card without reflashing**, and **typing a Wi-Fi password on the
device**. Neither has run on hardware yet.

Do **not** flash `2026-11-06-zpod-pi0w-v0.11.0-br-ab-test6.img.xz` onto the test5 card: a
full image wipes `/data` (music, Wi-Fi, accounts). The `.img.xz` is only for
a new or spare card.

## 1. Update test5 in place

On the Mac (zsh), one step at a time. Download
`zpod-v0.11.0-br-ab-test6.zab` from the release, then check it:

```
cd ~/Downloads && shasum -a 256 zpod-v0.11.0-br-ab-test6.zab
```

Compare with `SHA256SUMS` in the release. Then, from the repo:

```
cd ~/cb/projects/zpod && git pull
```

```
scripts/zpod-ab-push --verify-only ~/Downloads/zpod-v0.11.0-br-ab-test6.zab
```

Expect `ok: v0.11.0-br-ab-test6 <git>`. Install (writes p3 and the boot partition only;
`/data` is not touched; no reboot):

```
scripts/zpod-ab-push ~/Downloads/zpod-v0.11.0-br-ab-test6.zab
```

Expect "root file system written and verified", "copied the running
system's boot files to /boot/firmware/slot-a/" (test5 migration) and "next
boot runs system b (v0.11.0-br-ab-test6) on trial". Then on the ZPOD: **Power > Reboot**.

Check (over SSH):

```
scripts/zpod-ab-push --status
```

Wait 2-3 minutes after boot: expect
`slot: b`, `result: system b (v0.11.0-br-ab-test6 ...) committed`, and the music and
Wi-Fi unchanged.

Please report: did it boot system B? Committed? Boot time? Anything in
`ab.log`?

### Fallback checks (optional, after a commit)

- **Square held while switching on** -> test5 (system A) boots; `zpod-ab
  status` shows `slot: a`. Next normal boot is B again.
- If the ZPOD never shows the screen after the reboot: switch off, hold
  **Square**, switch on -> test5. If that fails too: card into the Mac,
  delete `zpod-slot.txt` on the `ZPODBOOT` drive, eject, boot -> test5.
- Going back to test5 for good: `zpod-ab switch` on the device, reboot.

## 2. Wi-Fi password entry

Wi-Fi > Networks nearby > a network with a lock > OK (details) > OK = Join.

- **Buttons only**: arrows + OK pick characters; Square = next set (abc, ABC,
  123, #+=); Triangle = show/hide; X = delete (cancel when empty); Done =
  join. Dots by default.
- **USB keyboard** (port in host mode, keyboard plugged in): type, Enter =
  join, Esc = cancel. Pull the keyboard out mid-entry -> the grid comes back
  with the text kept.
- In gadget mode the grid has a `kbd` cell: it asks to switch the port to
  host mode (the USB link to the Mac goes down; Wi-Fi/SSH stay).
- A wrong password must say so and leave the previous network connected.
  "Joined" only appears with an IP.

Report: easy to type? Readable? Anything slow?
