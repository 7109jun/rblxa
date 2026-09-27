# RBLXA

RBLXA is a Windows-only Rust compiler for a TOML-like Roblox Place authoring format.

```text
.rblxa -> RBLXA Compiler -> .rbxl
```

## Commands

```text
rblxa build <input.rblxa> [-o output.rbxl]
rblxa check <input.rblxa>
rblxa format <input.rblxa> [--write]
rblxa inspect <input.rblxa>
rblxa inspect <input.rbxl>
rblxa version
```

## Stage status

- 1-7: feature implementation complete
- 8-15: bug fixing, optimization, integration, stabilization
- 15/15: final stabilization complete

## Platform

Windows x64 only (`x86_64-pc-windows-msvc`).

## Final status

RBLXA v1.0 feature set is frozen. Stages 8-15 contain only bug fixes, optimization, regression coverage, integration, and stabilization.

The compiler is Windows x64 only (`x86_64-pc-windows-msvc`).
