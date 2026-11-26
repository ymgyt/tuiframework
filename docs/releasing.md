# Releasing tuiframework

```sh
just release <level> [--execute]
```

When you push a release tag, GitHub Actions runs `just check`, verifies the package, and publishes it to crates.io.
See the [CD workflow](../.github/workflows/cd.yaml).

Trusted Publishing is enabled for this crate.
