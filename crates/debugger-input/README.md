# sBPF Debugger Input

This crate provides functionality for generating serialized debugger input to debug sBPF Assembly programs using the sBPF Debugger.

## Usage

```rust
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use sbpf_dbg_input::generate;

fn main() {
    let program_id = Pubkey::new_unique();
    let owner_pubkey = Pubkey::new_unique();
    let vault_pda = Pubkey::new_unique();

    let instruction = Instruction::new_with_bytes(
        program_id,
        &[1, 2, 3, 4], // instruction data
        vec![
            AccountMeta::new(owner_pubkey, true),
            AccountMeta::new(vault_pda, false),
        ],
    );

    // Generate debugger input
    generate(
        &instruction,
        "input",
    )?;
}

```

### Notes

1. The input (.hex) files are generated inside a .dbg folder in the workspace.
2. You can either create a separate Rust script to generate the input files or integrate the logic into your existing Rust tests (see example [here](../../extension/sample/src/lib.rs)).