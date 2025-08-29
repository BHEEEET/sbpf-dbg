# sBPF Debugger Input

This crate provides functionality for generating serialized input to debug sBPF Assembly programs using the sBPF Debugger.

## Serialization format

The input is serialized in the following format:

- 8 bytes: unsigned number of accounts
- For each account:
  - 1 byte: duplicate marker. If not a duplicate → 0xff; if duplicate → the index of the original account
  - If duplicate:
    - 7 bytes: padding
  - If not duplicate:
    - 1 byte: boolean, true if account is a signer
    - 1 byte: boolean, true if account is writable
    - 1 byte: boolean, true if account is executable
    - 4 bytes: padding
    - 32 bytes: account public key
    - 32 bytes: owner public key
    - 8 bytes: unsigned number of lamports
    - 8 bytes: unsigned number of bytes of account data
    - x bytes: account data
    - 10k bytes: padding, reserved for realloc growth
    - padding: enough zero bytes to align the current offset to a multiple of 16 bytes
    - 8 bytes: rent epoch
- 8 bytes: unsigned number of instruction data
- x bytes: instruction data
- 32 bytes: program id 

## Usage

Cargo.toml
```
[dev-dependencies]
sbpf-dbg-input = { git = "https://github.com/bidhan-a/sbpf-dbg" }
...
```

main.rs
```rust
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use sbpf_dbg_input::generate;

fn main() {
    let program_id = Pubkey::new_unique();
    let owner_pubkey = Pubkey::new_unique();
    let owner_account = Account::new(1000000000, 0, &program_id);

    let instruction = Instruction::new_with_bytes(
        program_id,
        &[1, 2, 3, 4], // instruction data
        vec![
            AccountMeta::new(owner_pubkey, true),
        ],
    );

    // Generate debugger input
    generate(
        &instruction,
        &[(owner_pubkey, owner_account)]
        "input",
    ).unwrap();
}

```

### Notes

1. The input (.hex) files are generated inside a .dbg folder in the workspace. Add this folder to .gitignore if desired.
2. You can either create a separate Rust script to generate the input file(s) or integrate the logic into your existing Rust tests (see example [here](../../extension/sample/src/lib.rs)).
3. Provide the input file using the `--input` argument when running the debugger in REPL mode. If you're using the VS Code extension, set it in the `input` configuration (see example [here](../../extension/sample/.vscode/launch.json)). 