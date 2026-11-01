# TinyLang v2 ABI workspace setup

The v1 source tree and ABI are frozen. Develop the v2 binary ABI in a sibling
Git worktree so uncommitted v1 work never follows the feature branch.

```powershell
Set-Location C:\Users\conne\Desktop\TinyOne\TinyOne
git fetch origin
git worktree add -b v2/abi-binary-protocol `
  C:\Users\conne\Desktop\TinyOne-v2-abi `
  origin/main
Set-Location C:\Users\conne\Desktop\TinyOne-v2-abi
git status --short --branch
```

All implementation files live below `tinylang_v2/`. The crate is deliberately
not added to an existing workspace or v1 module tree.

Run the implementations independently:

```powershell
cargo test --manifest-path .\tinylang_v2\Cargo.toml
cargo test --manifest-path .\TinyOne\Cargo.toml
```

Remove the worktree only after its changes are committed or otherwise saved:

```powershell
Set-Location C:\Users\conne\Desktop\TinyOne\TinyOne
git worktree remove C:\Users\conne\Desktop\TinyOne-v2-abi
```

