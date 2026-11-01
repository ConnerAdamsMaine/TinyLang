# TinyLang v2 golden vectors

These `.bin` files are canonical complete request frames. They are shared test
inputs for Rust, C/C++, Python, Node.js, and future bindings. `manifest.json`
records their byte lengths, SHA-256 digests, and expected boundary/response
statuses.

Regenerate them from the byte-oriented reference generator:

```powershell
cargo run --manifest-path .\tinylang_v2\Cargo.toml --example generate_vectors
```

After regeneration, `git diff --exit-code -- tinylang_v2/tests/vectors` must be
clean. A changed vector is a protocol change and requires schema-version review;
it must never be accepted as a formatting-only update.

