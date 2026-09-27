# Soroban Forge — Contract Template

Use the CLI to scaffold a new Soroban Forge contract crate:

```bash
cargo run -p soroban-forge-cli -- new my-contract
```

The generator writes a minimal, standalone contract crate with:

1. a `Cargo.toml` ready for `cargo check`
2. a `src/lib.rs` re-exporting the generated contract
3. a `src/contract.rs` implementing an example method
4. `src/errors.rs` containing a valid `ForgeError`
5. `src/types.rs` for domain-specific types

From there, customize the generated contract code and add tests as needed.
