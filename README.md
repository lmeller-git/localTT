# local-tt

A locally hosted D&D 5e campaign server and terminal client. The engine and
stores are synchronous; `localtt-server` owns one actor thread and exposes an
async façade that the TUI uses today and HTTP/WebSocket will use later.

## Run the TUI

```sh
cargo run -p localtt-server --bin localtt -- --create --name "My Campaign" --dm "DM"
```

This creates `<data>/campaigns/default.sqlite` in the platform local data
directory. Pass `--data DIR` or `--campaign FILE` to choose a location. Reopen
the same campaign by omitting `--create`.

Keys: `?` for help, `q` to quit, `j`/`k` to select, and the action keys shown in
the footer. A campaign has no characters yet; the current milestone creates the
campaign and the TUI shell. Character creation is the next slice.

## Checks

```sh
just fmt
just lint
just check
just test
```
