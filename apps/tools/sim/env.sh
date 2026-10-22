# Common simulator environment (source me). Outputs go to $ZPOD_SIM.
export ZPOD_SIM=${ZPOD_SIM:-/tmp/zpod-sim/out}
export ZPOD_FAKE_TIME="2026-10-17 10:08:42"
export ZPOD_SIM_BATT=87 ZPOD_SIM_VOL=72
export ZPOD_SIM_IP_WLAN0=192.168.1.50 ZPOD_SIM_IP_USB0=10.12.194.1
export ZPOD_SIM_SSID="HomeNet"
export ZPOD_DATA_DIR=/tmp/zpod-sim/data XDG_CACHE_HOME=/tmp/zpod-sim/cache
export ZPOD_APPS_DIR=/tmp/zpod-sim/apps
export MPD_HOST=${MPD_HOST:-/tmp/zpod-mpd/socket}
export ZPOD_SEED=7
