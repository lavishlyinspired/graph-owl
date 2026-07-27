# Scripts

One script, one URL: **http://localhost:8080**. There is no second port and no
second console.

An earlier `dev.sh` ran Vite on `:5173` alongside the server for hot reload.
It served the same `ui/src/`, but a second origin is a second everything —
notably `localStorage`, so the theme toggle on one port had no effect on the
other and the two could sit on different themes indefinitely. A faster edit
loop is not worth being unable to answer "which one am I looking at". Removed.

## `./scripts/demo.sh`

Everything from nothing: Postgres, the seeded bank estate, the console build,
the server, and a catalogue run. One command.

```
./scripts/demo.sh            # open — every request is the system principal
./scripts/demo.sh --secure   # JWT on, with two principals and the PII policy
./scripts/demo.sh --stop     # tear it down
```

`--secure` prints tokens for `root` (admin) and `asha` (risk analyst, denied
`core_banking`). Use them as `Authorization: Bearer <token>` to see the same
search return different results.

### Changing the console

The console is embedded in the binary by `rust-embed`, so a `.tsx` edit needs
`demo.sh` again to rebuild the bundle and relink. Slower than hot reload; the
tradeoff is that what you are looking at is always what ships.

### Theme

Light by default. The header toggle switches to dark and the choice persists.
`?theme=dark` / `?theme=light` overrides it, which is what makes a screenshot
reproducible from its URL.
