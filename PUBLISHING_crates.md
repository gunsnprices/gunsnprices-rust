# Publishing to crates.io

Crate: `gunsnprices` — https://crates.io/crates/gunsnprices (the name was free on 2026-11-10:
`https://crates.io/api/v1/crates/gunsnprices` returned 404 "crate `gunsnprices` does not exist").

After publishing:

- crate page: https://crates.io/crates/gunsnprices (sidebar: Homepage → https://gunsnprices.com, Repository,
  Documentation)
- API docs, built automatically by docs.rs from the published crate (all features, see
  `[package.metadata.docs.rs]`): https://docs.rs/gunsnprices — the build usually takes a few minutes; status at
  https://docs.rs/crate/gunsnprices/latest/builds
- lib.rs (an alternative index of crates.io, nothing to do): https://lib.rs/crates/gunsnprices — it picks the crate up
  on its own within a day.

Publishing is permanent: a version can't be deleted or overwritten. A broken version can be *yanked*
(`cargo yank --version 0.1.0`): existing `Cargo.lock` files keep working, new projects won't select it. The code and
data stay downloadable.

## 1. Account

crates.io has no own passwords: you sign in with a **GitHub account** at https://crates.io (top right, "Log in with
GitHub"). The account that publishes first becomes the owner of the crate. Use the account that owns (or is an owner of)
the `gunsnprices` GitHub organization.

1. Sign in at https://crates.io with GitHub and authorize the crates.io OAuth app.
2. Open https://crates.io/settings/profile, enter an e-mail address (for example app@gunsnprices.com) and click the
   link in the verification e-mail. **crates.io refuses to publish until the e-mail is verified.**
3. Optional, after the first publish: add the GitHub team as co-owner so the crate does not depend on one person:
   `cargo owner --add github:gunsnprices:<team-name> gunsnprices` (team owners can publish and yank, but not change
   owners).

## 2. API token

1. https://crates.io/settings/tokens → **New Token**.
2. Name: e.g. `gunsnprices-publish`; expiration: e.g. 90 days.
3. Scopes: **publish-new** (needed for the very first upload, which creates the crate) and **publish-update**. Leave
   `yank` and `change-owners` off unless you need them. Crate pattern: `gunsnprices`.
4. Copy the token (shown once) and run `cargo login` — it asks for the token and stores it in
   `~/.cargo/credentials.toml`. Don't commit it or put it in the repository.

After the first release you can replace this token with one that has only `publish-update`.

## 3. Publish

Requires Rust 1.71 or newer for building and a current stable toolchain for the checks, on a clean checkout of `main`
with the version in `Cargo.toml` and `CHANGELOG.md` already bumped. The GitHub repository
`gunsnprices/gunsnprices-rust` should exist and be public first: the crate page links to it.

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features                 # golden: 1,150/1,150 titles, README examples run as doctests
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo package --list                      # what goes into the .crate (see `include` in Cargo.toml)
cargo publish --dry-run                   # builds the packaged crate from scratch
cargo publish                             # uploads; needs the token from step 2
git tag v0.1.0 && git push origin v0.1.0
```

`cargo publish --dry-run` passed on 2026-11-10 (25 files, about 171 KiB compressed).

Checks crates.io does on upload that `--dry-run` can't: the e-mail must be verified, the token must have
`publish-new` for a new crate, and the `license` must be a valid SPDX expression — `MIT AND CC-BY-4.0` is (checked with
the `spdx` crate in strict mode; both identifiers are on the SPDX license list 3.29 and not deprecated). crates.io also
rejects more than 5 keywords and unknown category slugs; ours are within the limits (`text-processing`,
`parser-implementations`, `command-line-utilities` from https://crates.io/category_slugs).

## 4. Later releases from GitHub Actions (optional)

crates.io supports *trusted publishing* (OIDC), so GitHub Actions can publish without a stored token:

1. After the first manual publish, open https://crates.io/crates/gunsnprices/settings → **Trusted Publishing** → add
   GitHub, owner `gunsnprices`, repository `gunsnprices-rust`, workflow `publish.yml`.
2. Add `.github/workflows/publish.yml`:

   ```yaml
   name: Publish to crates.io
   on:
     push:
       tags: ['v[0-9]+.[0-9]+.[0-9]+*']
   jobs:
     publish:
       runs-on: ubuntu-latest
       permissions:
         id-token: write
       steps:
         - uses: actions/checkout@v4
         - uses: dtolnay/rust-toolchain@stable
         - uses: rust-lang/crates-io-auth-action@v1
           id: auth
         - run: cargo publish
           env:
             CARGO_REGISTRY_TOKEN: ${{ steps.auth.outputs.token }}
   ```

3. Release: bump the version in `Cargo.toml` and `CHANGELOG.md`, commit, `git tag vX.Y.Z && git push origin vX.Y.Z`.

## Mirrors

- **docs.rs**: builds every published version automatically; nothing to register. If a build fails, fix and publish a
  new patch version (docs.rs can't rebuild a version from changed sources).
- **lib.rs**: indexes crates.io automatically.
- The `gunsnprices` binary is installed from source with `cargo install gunsnprices --features cli`; no separate binary
  release is needed.
