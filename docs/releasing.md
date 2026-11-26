# Releasing tuiframework

## First release

crates.io accepts a Trusted Publisher configuration after the first version is published.
For `0.1.0`, start from the clean commit on `main` that you want to release.
Verify the package from the repository root.

```sh
just publish-dry-run
```

Publish with `CARGO_REGISTRY_TOKEN` set to a token that grants `publish-new` for `tuiframework`.

```sh
just publish
```

After Cargo reports that the crate is published, tag the same commit.
The release workflow excludes this first tag because the package is already on crates.io.

```sh
git tag v0.1.0
git push origin refs/tags/v0.1.0
```

In the [crate settings](https://crates.io/crates/tuiframework/settings), add a GitHub Actions
Trusted Publisher. Set the repository owner to `ymgyt`, the repository name to `tuiframework`,
and the workflow filename to `release.yaml`. Leave the environment field empty.
Revoke the first-release token in the [crates.io token settings](https://crates.io/settings/tokens).

## Subsequent releases

Commit the new version in `Cargo.toml` and the matching `Cargo.lock` on `main`.
Push a tag for that commit matching `v<version>`. For example, for version `0.1.1` run

```sh
git tag v0.1.1
git push origin refs/tags/v0.1.1
```

The [Release workflow](../.github/workflows/release.yaml) checks the tag against `Cargo.toml`,
runs `just check` and `just publish-dry-run`, then obtains a short-lived crates.io token
through GitHub OIDC and runs `just publish`. Running the workflow manually ends after the dry run.
