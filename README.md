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

Press `n` to create a player character: name, class, level, maximum HP and six
ability scores in STR,DEX,CON,INT,WIS,CHA order (`standard` fills
15,14,13,12,10,8). The form applies a small default character; the campaign
store is written when the command commits. `N` creates a handout (`text` or an
`item` whose content becomes the inventory item name), `f` gives the selected
handout to a recipient, `i`/`u`/`y` add, edit or remove an inventory item, and
the selected statblock shows its derived sheet and inventory. `?` shows the
other keys, `j`/`k` select an entity, and `q` quits.

## Checks

```sh
just fmt
just lint
just check
just test
```
