# local-tt

A locally hosted D&D 5e campaign server and terminal client. The command logic
stays synchronous; `localtt-server` runs it on one actor thread with a
current-thread Tokio runtime and an async store path, behind a façade shared by the
TUI and the REST adapter. The transport contract lives in
`localtt-server::protocol`; the REST API and `/ws` event stream are built, and the
browser client is next.

## Run the TUI

```sh
cargo run -p localtt-server --bin localtt -- --create --name "My Campaign" --dm "DM"
```

This creates `<data>/campaigns/default.sqlite` in the platform local data
directory. Pass `--data DIR` or `--campaign FILE` to choose a location. Reopen
the same campaign by omitting `--create`.

Press `n` to create a player character: name, class, level, maximum HP and six
ability scores in STR,DEX,CON,INT,WIS,CHA order (`standard` fills
15,14,13,12,10,8). The form applies a small default character; the campaign
store is written when the command commits. `N` creates a handout (`text` or an
`item` whose content becomes the inventory item name), `f` gives the selected
handout to a recipient, `i`/`u`/`y` add, edit or remove an inventory item, and
the selected statblock shows its derived sheet and inventory. Press `/` to
search the reference content: type a name, press Enter, select a hit with
`j`/`k`, and press Enter again to read the entry. Content uses `<data>/reference`
when present and otherwise the shipped SRD snapshot. `?` shows the other keys,
`j`/`k` select an entity, and `q` quits.

## Serve the API

```sh
cargo run -p localtt-server --bin localtt -- serve --create --name "My Campaign" --dm "DM"
```

The server binds `127.0.0.1:8787` by default and prints a per-run bearer token.
Use `Authorization: Bearer <token>` on `GET /api/v1/health`, `GET /api/v1/campaign`,
`GET /api/v1/entities`, `POST /api/v1/commands`, the reference routes and the rest
of the table in `docs/rfcs/server.md` section 7. The token is also written to
`<data>/server.token` owner-only (`0600` on Unix); pass `--no-token-file` to
disable the file. `GET /ws?token=<token>` streams `StreamEventDto` JSON text for
the authenticated viewer; a browser cannot set an Authorization header on a
WebSocket, so it uses the query token. `localtt tui --serve` runs the TUI and API
together. Bind another address with `--bind`, and allow a browser origin with
`--origin http://localhost:5173`.

## Checks

```sh
just fmt
just lint
just check
just test
```
