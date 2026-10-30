# First login: creating the user (and SSH key) before first boot

ZPOD images contain **no password and no SSH key**. The first account is
`pi` (uid 1000) with a locked password and `/usr/sbin/nologin`, waiting for
Raspberry Pi OS's first-boot rename (`userconfig.service`, package
`userconf-pi`). Until that runs, SSH prints

    Please note that SSH may not work until a valid user has been set up.
    See http://rptl.io/newuser for details.

and every login fails ("Permission denied"), for `pi` and for your name.
The ZPOD has no keyboard (the data port is a USB gadget), so the HDMI wizard
cannot be used: put the setup on the boot partition (volume **bootfs** on the
Mac) after writing the image, before (or between) boots.

> **Add every bootfs file before the first power-on.** Write `userconf.txt`,
> `zpod-ssh.pub`, `zpod-user.txt` and `zpod-wifi.txt` in one go, eject, and only
> then boot. A boot with `zpod-ssh.pub` but no `userconf.txt` finishes a
> **key-only** setup (see below); on `v0.6.0-m5-rc1` and earlier a
> `userconf.txt` added after that is ignored (see
> [Recovery](#recovery-key-only-first-boot-then-userconftxt-v060-m5-rc1-and-earlier)).

## Quick setup (cut/paste, on the Mac; volume `bootfs`)

Two steps. Paste **Step 1**, enter the ZPOD login password when OpenSSL asks (twice), then paste **Step 2**.

Needs OpenSSL 3 (`openssl passwd -6`). macOS `/usr/bin/openssl` has no `-6`; use conda or Homebrew OpenSSL. Required unless you use a key-only setup (`zpod-ssh.pub` only).

**Step 1** — login user + password:

```bash
H=$(openssl passwd -6) && case "$H" in '$6$'*) printf 'paul:%s\n' "$H" > /Volumes/bootfs/userconf.txt ;; *) echo "use OpenSSL 3 (e.g. conda); macOS openssl has no -6"; exit 1 ;; esac; unset H
head -c 9 /Volumes/bootfs/userconf.txt; echo
```

**Step 2** — SSH key, Wi-Fi, eject (no password prompt):

```bash
cp ~/.ssh/id_ed25519.pub /Volumes/bootfs/zpod-ssh.pub
printf 'SSID=%s\nPSK=%s\n' '<SSID>' '<PW>' > /Volumes/bootfs/zpod-wifi.txt
ls /Volumes/bootfs/userconf.txt /Volumes/bootfs/zpod-ssh.pub /Volumes/bootfs/zpod-wifi.txt
diskutil eject /Volumes/bootfs
```

Replace `<SSID>` and `<PW>` with your network name and password before running Step 2. Wi-Fi is optional (USB networking works without it; the file is imported at first boot then deleted). SSH key is optional (public key only).

Then `ssh paul@zpod.local` (Wi-Fi) or `ssh paul@10.12.194.1` (USB cable on the data port). To add music, run `scripts/zpod-push-music.sh ~/Music/Artist`, or open `http://10.12.194.1:8080/` over USB.

Details of each file follow.

## Password login (all images): `userconf.txt`

```sh
H=$(openssl passwd -6) && case "$H" in '$6$'*) printf 'paul:%s\n' "$H" > /Volumes/bootfs/userconf.txt ;; *) echo "no SHA-512 hash: use OpenSSL 3 (brew install openssl)" ;; esac; unset H
head -c 9 /Volumes/bootfs/userconf.txt; echo     # expect: paul:$6$.
```

`openssl passwd -6` asks for the password twice; only the hash is written.
macOS's own `/usr/bin/openssl` (LibreSSL) has **no `-6`** ("unknown option"):
use Homebrew/MacPorts/conda OpenSSL 3. A file the service cannot parse is
renamed `failed_userconf.txt` and the account stays locked.

At boot `userconfig.service` renames `pi` to `paul` (home moves to
`/home/paul`), sets the password, removes the SSH banner and deletes
`userconf.txt`. Then `ssh paul@zpod.local` (or `paul@10.12.194.1` over USB)
works, and `ssh-copy-id paul@zpod.local` installs your key.

## SSH key (M4.1+ images): `zpod-ssh.pub`

```sh
cp ~/.ssh/id_ed25519.pub /Volumes/bootfs/zpod-ssh.pub     # public key only
echo paul > /Volumes/bootfs/zpod-user.txt                  # optional login name for key-only setup
```

`zpod-firstuser.service` (before `userconfig.service`) appends the public
key(s) to `~/.ssh/authorized_keys` of uid 1000 and deletes `zpod-ssh.pub`.
Lines that are not public keys are ignored; a file containing a private key is
refused.

* With `userconf.txt` too: name and password come from `userconf.txt`; the key
  is installed as well (it moves with the home directory on rename).
* **Key-only** (no `userconf.txt`): the account is finished for SSH key login
  as `zpod-user.txt`'s name (default `pi`). It has no password, so sudo is
  passwordless for it (like stock Raspberry Pi OS `010_pi-nopasswd`, file
  `/etc/sudoers.d/010_zpod-nopasswd`). Set a password later with
  `sudo passwd paul` if you want one.
  **Always add `zpod-user.txt` with your name** (`echo paul >
  /Volumes/bootfs/zpod-user.txt`) for key-only cards, so the login is `paul`
  and not `pi`.
* **`userconf.txt` added after a key-only boot** (images after
  `v0.6.0-m5-rc1`): `zpod-firstuser.service` also runs when only
  `userconf.txt` is on bootfs. If the card was set up key-only (the
  `010_zpod-nopasswd` file exists), it renames the user to `userconf.txt`'s
  name, sets its password, deletes `userconf.txt` and removes
  `/etc/sudoers.d/010_zpod-nopasswd` (sudo then asks for the password). An
  invalid file is renamed `failed_userconf.txt`. A card whose user was set up
  with a password is not changed by a later `userconf.txt`.
* Adding `zpod-ssh.pub` later (already set-up card) just adds the key.

## Recovery: card already booted without a user (no reflash)

1. Shut the ZPOD down, put the card in the Mac. Check what is on bootfs:
   `ls /Volumes/bootfs | grep -iE 'userconf|firstrun|user-data|network-config|zpod-'`
   (`failed_userconf.txt` = the earlier file was invalid; `user-data` /
   `network-config` = Raspberry Pi Imager cloud-init customisation, which this
   image ignores: it has no cloud-init).
2. Write `userconf.txt` as above (and on M4.1+ images `zpod-ssh.pub`).
3. `diskutil eject /Volumes/bootfs`, boot, wait ~2 min, `ssh paul@zpod.local`.

The rename only waits for the files: `userconfig.service` stays enabled until a
user is set up, so this works on any boot (but see the next section if the
card already did a key-only setup).

## Recovery: key-only first boot, then `userconf.txt` (`v0.6.0-m5-rc1` and earlier)

Symptom: the card booted once with `zpod-ssh.pub` but without a valid
`userconf.txt`; `userconf.txt` was added later, and `ssh paul@…` fails while
`ssh pi@…` works with the key. The key-only setup ended the rename wizard
(removed `/etc/ssh/sshd_config.d/rename_user.conf`, disabled
`userconfig.service`), so `userconf.txt` was never read. Put a valid
`userconf.txt` on bootfs (Step 1 above), boot, then reopen the wizard and
reboot (key login as `pi`, passwordless sudo, no password prompt):

```sh
ssh pi@10.12.194.1 'sudo touch /etc/ssh/sshd_config.d/rename_user.conf && sudo systemctl enable userconfig && sudo reboot'
```

`userconfig.service` then renames `pi` to `paul` and sets the password from
`userconf.txt`. Afterwards remove the leftover passwordless-sudo file, which
still names `pi` (harmless, but no longer needed; sudo asks for paul's
password):

```sh
ssh -t paul@10.12.194.1 'sudo rm -f /etc/sudoers.d/010_zpod-nopasswd'
```

Images after `v0.6.0-m5-rc1` do this automatically at the next boot with
`userconf.txt` present (see above).

## Storage layout (M5 images)

M5 images have three partitions: boot (512 MiB FAT, the volume **bootfs**
where the files above go), a fixed-size root, and **`/data`**, which grows to
fill the card at first boot (up to 2 TiB). Music, playlists and app settings
live on `/data`; the paths you use (`/srv/zpod/media/music`) are unchanged.
The first-boot files (`userconf.txt`, `zpod-ssh.pub`, `zpod-user.txt`,
`zpod-wifi.txt`, `zpod-usb.txt`) work exactly as above. See
[STORAGE.md](STORAGE.md).

## Status on the device

Settings > Battery & about shows `login paul`, or `NOT SET UP` while the first
user is still unconfigured; Settings > Diagnostics > Storage & build shows the
same on its **Login** row.
