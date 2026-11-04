#!/bin/sh
# Defaults the container to a headless server that serves the built browser
# client. Pass "serve" as the first argument to supply the full server command
# yourself; extra arguments are appended to the generated command.

set -eu

if [ "${1:-}" = "serve" ]; then
    exec /usr/local/bin/localtt "$@"
fi

data_dir="${LOCALTT_DATA_DIR:-/data}"
campaign="${LOCALTT_CAMPAIGN:-${data_dir}/campaigns/default.sqlite}"
bind="${LOCALTT_BIND:-0.0.0.0:8787}"
web_dir="${LOCALTT_WEB_DIR:-/usr/local/share/localtt/web}"

create=0
have_name=0
have_dm=0
for arg in "$@"; do
    case "$arg" in
        --create) create=1 ;;
        --name) have_name=1 ;;
        --dm) have_dm=1 ;;
    esac
done

set -- serve \
    --data "$data_dir" \
    --campaign "$campaign" \
    --bind "$bind" \
    --web-dir "$web_dir" \
    "$@"

lan=0
case "$bind" in
    127.* | localhost* | \[::1\]*) ;;
    *)
        lan=1
        set -- "$@" --lan
        ;;
esac

if [ "$lan" -eq 1 ] && [ -n "${LOCALTT_LAN_HOST:-}" ]; then
    set -- "$@" --lan-host "$LOCALTT_LAN_HOST"
fi

if [ -n "${LOCALTT_ORIGINS:-}" ]; then
    old_ifs=$IFS
    IFS=,
    for origin in $LOCALTT_ORIGINS; do
        set -- "$@" --origin "$origin"
    done
    IFS=$old_ifs
fi

if [ ! -f "$campaign" ]; then
    if [ "$create" -eq 0 ]; then
        set -- "$@" --create
    fi
    if [ "$have_name" -eq 0 ]; then
        set -- "$@" --name "${LOCALTT_CAMPAIGN_NAME:-New Campaign}"
    fi
    if [ "$have_dm" -eq 0 ]; then
        set -- "$@" --dm "${LOCALTT_DM_NAME:-Dungeon Master}"
    fi
fi

exec /usr/local/bin/localtt "$@"
