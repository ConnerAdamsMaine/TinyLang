# TinyLang v2 binary artifact vectors

These files are canonical `TLBC` profile-0 containers shared by the loader,
bindings, and artifact fuzz target. Their SHA-256 digests and expected loader
statuses are recorded in `manifest.json`.

Regenerate both wire and artifact vectors with:

```powershell
cargo run -p tinylang-v2 --example generate_vectors
```

A changed digest is an artifact-format change and requires explicit versioning
review. It must not be accepted as a mechanical regeneration.

