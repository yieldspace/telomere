# Releasing

## Current release boundary

Telomere is not published to crates.io. A Git tag is the release artifact;
publishing packages is a separate maintainer decision. The workspace keeps
`publish = false` until that decision is made.

The eight workspace crates (`telomere-cli` and the seven members under
`crates/`) share `workspace.package.version`. Release them in lockstep; do not
give an individual crate a different release version.

## Versioning before 1.0

Until a stable 1.0 release, use `0.MINOR.PATCH` versions:

| Segment | Use it for |
| --- | --- |
| `MINOR` | A breaking public Rust API change, a feature-gate rename or removal, a CLI flag removal, or an MSRV increase. |
| `PATCH` | Fixes and additive changes. |

These rules organize release communication only. Before 1.0, Telomere makes no
compatibility guarantee.

## Tags

Create an annotated Git tag in this form:

```text
vMAJOR.MINOR.PATCH[-PRERELEASE]
```

For example, the first planned prerelease tag is `v0.1.0-alpha.1`. After a tag
has been pushed, do not move or delete it. If it needs correction, publish a
new tag with an accurate changelog rather than rewriting the old tag.

## Changelog policy

[CHANGELOG.md](CHANGELOG.md) follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/):
`[Unreleased]` stays at the top, and entries use `Added`, `Changed`, `Fixed`,
`Removed`, and `Security` sections as applicable.

At release time, prepare the entry from the titles of merged pull requests and
edit them into useful release notes. A changelog entry on every individual pull
request is not required.

## Release gate

The exact tagged commit must be on `main` and green for all of these checks:

- `ci.yaml`: `workspace-test`, `minimal-embedder`, all three
  `jit-native-test` targets, `jit-cross-check`, and `packaging`;
- `lint.yaml`: `clippy` and `rustfmt`.

This is a release gate, not a statement about branch-protection configuration.

## Release checklist

Copy this checklist when preparing a tagged release:

1. Choose the planned tag and set the release heading date in `CHANGELOG.md`
   to that planned tag date. If the release PR's merge date moves, update the
   heading before merging the PR.
2. Bump the workspace version so that all eight crates remain in lockstep. For
   a prerelease, include the prerelease identifier in that version so the
   packaging checker produces the matching tag.
3. Refresh the lockfile:

   ```shell
   cargo check --workspace
   ```

4. Complete the changelog entry from the merged PR titles.
5. Open, review, and merge the release PR; wait for the release-gate CI and
   lint checks to be green on the commit that will be tagged.
6. Create and push the annotated tag:

   ```shell
   TAG="v$(python3 tools/check-packaging.py --print-version)"
   git tag -a "$TAG" -m "$TAG"
   git push origin "$TAG"
   ```

7. Create the GitHub release using the corresponding changelog entry.

## Deprecations

When practical, retain a public Rust API behind `#[deprecated]` for one
`MINOR` release before removing it. The experimental JIT is an exception: its
surfaces may change without that deprecation period, but the change must be
recorded in the changelog.

## Future crates.io publishing

Crates.io publishing is deferred until a maintainer explicitly decides to do
it. [`tools/check-packaging.py`](tools/check-packaging.py) is the current
source of truth for package publication intent: its `EXPECTED` mapping sets all
eight workspace crates to `False`.

Before changing any `publish` setting or running `cargo publish`, a maintainer
must at least:

- make the maintainer decision and update each affected `EXPECTED` entry in
  `tools/check-packaging.py` to its actual publish value: `True` for
  unrestricted publication or an explicit registry list for restricted
  publication;
- update `README.md`, `SECURITY.md`, and `CONTRIBUTING.md` so their availability,
  support, and contributor guidance match the decision;
- add appropriate `repository` and `readme` package metadata to the packages
  being published;
- audit Cargo `include`/`exclude` behavior with the package file list. In
  particular, verify that every input read by
  `crates/telomere-component-wasi/build.rs` is included; and
- audit the `crates/telomere/tests/wasm-testsuite` submodule so it is neither
  unexpectedly included nor required by a published package without an
  intentional packaging decision.
