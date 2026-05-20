# WASI Preview 3 WIT Snapshot

This directory vendors the official `WebAssembly/WASI` `main` snapshot checked
on 2026-05-19 for:

- `proposals/cli/wit-0.3.0-draft`
- `proposals/clocks/wit-0.3.0-draft`
- `proposals/filesystem/wit-0.3.0-draft`
- `proposals/random/wit-0.3.0-draft`
- `proposals/sockets/wit-0.3.0-draft`

The observed package version is `0.3.0-rc-2026-03-15`.

`../wit/` intentionally remains the WASI 0.2.6 `wasip2` WIT used to generate
the existing compatibility provider bindings. The Preview 3 provider is wired
manually against this snapshot while the async Component Model surface is still
behind explicit support-matrix entries.
