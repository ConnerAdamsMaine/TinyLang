# TinyLang v2 ABI workspace setup

The v1 ABI remains frozen on `release/v1`. Develop the experimental v2 binary
ABI from `main`, optionally in a sibling worktree to isolate local changes.
Run the setup from the primary repository root:

```powershell
$mainCheckout = (Get-Location).Path
$abiCheckout = Join-Path (Split-Path $mainCheckout) 'tinylang-v2-abi'
git fetch origin
git worktree add -b v2/abi-binary-protocol `
  $abiCheckout `
  origin/main
Set-Location $abiCheckout
git status --short --branch
```

The v2 scalar engine (`tinylang::engine`), binary ABI (`tinylang::v2`), and
`tinylang-v2` CLI are part of this crate and build in the root workspace.

Run the implementations independently:

```powershell
cargo test -p tinylang-v2
cargo test --workspace --all-features
```

Remove the worktree only after its changes are committed or otherwise saved:

```powershell
Set-Location $mainCheckout
git worktree remove $abiCheckout
```
