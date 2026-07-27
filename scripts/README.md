# Scripts

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

## `./scripts/dev.sh`

Frontend work with hot reload. The server runs on `:8080`, Vite on `:5173` with
`/api` proxied. Save a `.tsx` and the console reloads.

**Why a separate script**: the console is embedded in the binary via
`rust-embed`, so the `demo.sh` path needs a full Rust rebuild to see a frontend
change. `dev.sh` bypasses the embed entirely. Run `demo.sh` first to seed
Postgres, then `dev.sh` alongside it.
