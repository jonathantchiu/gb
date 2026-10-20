# gb-tests

ROM oracle harness. `cargo test -p gb-tests` runs the fast tier; slow ROMs
(`dmg_sound`, `cgb_sound`) need `--features slow`.

## Ratchet

`ratchet.txt` has one line per vendored ROM:

```
<status> <rom path> [timeout=<M-cycles>] [; reason]
```

- `pass` / `fail` — emulation result
- `not-applicable` — wrong hardware revision, or requires the owner's real
  boot ROM (`bootroms/`, gitignored); excluded from the denominator
- `timeout` — never reported within its emulated-cycle budget

Every `fail`→`pass` flip must carry a one-line `; reason` in the same
commit. Record reality with
`cargo run -p gb-tests --release --bin ratchet -- --update` (review the diff).

Timeouts are emulated cycles, not wall time: the suite is deterministic.
Each run also prints an FNV-1a fingerprint (framebuffer + registers) via
the `ratchet` binary for cross-run determinism checks; it is not an oracle.

## Oracles

- `mooneye/**`, `age/**` — LD B,B fingerprint (B,C,D,E,H,L = 3,5,8,13,21,34)
- `dmg-acid2`, `cgb-acid2` — framebuffer vs decoded upstream reference PNG
  (canonical format RGBA8888; DMG greys FF/AA/55/00; CGB no color correction)
- everything else — blargg serial text or cart-RAM signature
