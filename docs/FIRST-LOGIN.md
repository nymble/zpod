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
user is set up, so this works on any boot.

## Status on the device

Settings > Battery & about shows `login paul`, or `NOT SET UP` while the first
user is still unconfigured; Settings > Diagnostics > Storage & build shows the
same on its **Login** row.
