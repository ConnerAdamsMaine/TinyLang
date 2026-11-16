# Tynra Test Support

Shared workspace-location and error-assertion helpers for Tynra tests.
This crate is a development dependency, not part of the language runtime.
It is intended to be published so the other Tynra crate archives can resolve their
test dependencies without a sibling source checkout.

```toml
[dev-dependencies]
tynra_test_support = "0.1.0"
```

The workspace helpers walk upward from a manifest directory and honor
`CARGO_WORKSPACE_DIR` and `CARGO_TARGET_DIR` where documented. An enclosing
workspace takes precedence; unpacked standalone crates use their own
package root and target directory.

## License

See `LICENSE` for the MIT license.
