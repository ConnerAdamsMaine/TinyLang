# Tynra Ralloc

Fixed-capacity arena allocation and runtime storage for Tynra.
The package is `tynra_ralloc`; the Rust library and C library retain
the allocator name `ralloc`.

```toml
[dependencies]
ralloc = { package = "tynra_ralloc", version = "0.2.0" }
```

The crate builds Rust, dynamic C, and static C libraries. The C interface
is included at `include/ralloc.h`. The `testing-hooks` feature exposes
allocator test instrumentation and is not enabled by default.

See the [project documentation](https://github.com/ConnerAdamsMaine/TinyOne/tree/main/docs).

## License

See `LICENSE` for the MIT license.
