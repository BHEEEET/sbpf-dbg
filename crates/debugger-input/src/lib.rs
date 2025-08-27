use solana_sdk::account::Account as SolAccount;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use std::{
    fs::{create_dir_all, File},
    io::Write,
    mem::size_of,
    path::Path,
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DebuggerInputError {
    #[error("Failed to write to file: {0}")]
    FileWriteError(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Missing account data for pubkey {0}")]
    MissingAccount(Pubkey),
}

/// Constants for alignment and memory management
const BPF_ALIGN_OF_U128: usize = 16;
const MAX_PERMITTED_DATA_INCREASE: usize = 10240; // 10k bytes

/// Marker for non-duplicate accounts
const NON_DUP_MARKER: u8 = 0xff;

/// Simple serializer that just writes bytes to a buffer
struct Serializer {
    buffer: Vec<u8>,
}

impl Serializer {
    fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    fn write<T>(&mut self, value: T) {
        let bytes =
            unsafe { std::slice::from_raw_parts(&value as *const T as *const u8, size_of::<T>()) };
        self.buffer.extend_from_slice(bytes);
    }

    fn write_all(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    fn write_account_data(&mut self, data: &[u8]) {
        // Write actual data.
        self.write_all(data);

        // Add padding for realloc.
        self.buffer
            .extend(std::iter::repeat(0u8).take(MAX_PERMITTED_DATA_INCREASE));

        // Align to BPF_ALIGN_OF_U128.
        let current_len = self.buffer.len();
        let alignment_needed =
            (BPF_ALIGN_OF_U128 - (current_len % BPF_ALIGN_OF_U128)) % BPF_ALIGN_OF_U128;
        self.buffer
            .extend(std::iter::repeat(0u8).take(alignment_needed));
    }

    fn finish(self) -> Vec<u8> {
        self.buffer
    }
}

/// Account
pub struct Account {
    pub key: Pubkey,
    pub owner: Pubkey,
    pub lamports: u64,
    pub data: Vec<u8>,
    pub is_signer: bool,
    pub is_writable: bool,
    pub executable: bool,
    pub rent_epoch: u64,
}

impl Account {
    pub fn new(
        key: Pubkey,
        owner: Pubkey,
        lamports: u64,
        data: Vec<u8>,
        is_signer: bool,
        is_writable: bool,
        executable: bool,
        rent_epoch: u64,
    ) -> Self {
        Self {
            key,
            owner,
            lamports,
            data,
            is_signer,
            is_writable,
            executable,
            rent_epoch,
        }
    }
}

/// Account for serialization
pub enum SerializeAccount {
    Account(usize, Account),
    Duplicate(u8),
}

/// Serialize parameters into the expected format.
pub fn serialize_parameters(
    accounts: Vec<SerializeAccount>,
    instruction_data: &[u8],
    program_id: &Pubkey,
) -> Result<Vec<u8>, DebuggerInputError> {
    let mut s = Serializer::new();

    // Serialize into the buffer
    s.write::<u64>((accounts.len() as u64).to_le());

    for account in accounts {
        match account {
            SerializeAccount::Account(_, account) => {
                s.write::<u8>(NON_DUP_MARKER);
                s.write::<u8>(account.is_signer as u8);
                s.write::<u8>(account.is_writable as u8);
                s.write::<u8>(account.executable as u8);
                s.write_all(&[0u8, 0, 0, 0]); // 4 bytes padding
                s.write_all(account.key.as_ref());
                s.write_all(account.owner.as_ref());
                s.write::<u64>(account.lamports.to_le());
                s.write::<u64>((account.data.len() as u64).to_le());
                s.write_account_data(&account.data);
                s.write::<u64>(account.rent_epoch.to_le());
            }
            SerializeAccount::Duplicate(position) => {
                s.write::<u8>(position as u8);
                s.write_all(&[0u8, 0, 0, 0, 0, 0, 0]); // 7 bytes padding
            }
        };
    }

    s.write::<u64>((instruction_data.len() as u64).to_le());
    s.write_all(instruction_data);
    s.write_all(program_id.as_ref());

    Ok(s.finish())
}

/// Generate debugger input from a Solana instruction and write to file.
pub fn generate(
    instruction: &Instruction,
    accounts: &[(Pubkey, SolAccount)],
    output_name: &str,
) -> Result<(), DebuggerInputError> {
    // Convert AccountMeta to SerializeAccount with duplicate detection.
    let mut serialized_accounts = Vec::new();
    let mut seen_pubkeys = std::collections::HashMap::new();
    let by_pubkey: std::collections::HashMap<Pubkey, &SolAccount> =
        accounts.iter().map(|(k, v)| (*k, v)).collect();

    for (i, account_meta) in instruction.accounts.iter().enumerate() {
        if let Some(&first_index) = seen_pubkeys.get(&account_meta.pubkey) {
            // This is a duplicate account.
            serialized_accounts.push(SerializeAccount::Duplicate(first_index as u8));
        } else {
            // This is the first occurrence of this account.
            seen_pubkeys.insert(account_meta.pubkey.clone(), i);

            // Find provided account data by pubkey.
            let provided = by_pubkey
                .get(&account_meta.pubkey)
                .ok_or(DebuggerInputError::MissingAccount(account_meta.pubkey))?;

            let account = Account::new(
                account_meta.pubkey,
                provided.owner,
                provided.lamports,
                provided.data.clone(),
                account_meta.is_signer,
                account_meta.is_writable,
                provided.executable,
                provided.rent_epoch,
            );
            serialized_accounts.push(SerializeAccount::Account(i, account));
        }
    }

    // Serialize the parameters.
    let serialized_data = serialize_parameters(
        serialized_accounts,
        &instruction.data,
        &instruction.program_id,
    )?;

    // Ensure .dbg directory exists and create output file inside it.
    let out_dir = Path::new(".dbg");
    create_dir_all(out_dir)?;
    // Append .hex if not provided by the user.
    let output_name = if Path::new(output_name).extension().is_none() {
        format!("{}{}", output_name, ".hex")
    } else {
        output_name.to_string()
    };
    let output_path = out_dir.join(output_name);
    // Write hex to file.
    let mut file = File::create(output_path)?;
    for byte in &serialized_data {
        write!(file, "{:02x}", byte)?;
    }
    writeln!(file)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::{instruction::AccountMeta, pubkey::Pubkey};

    #[test]
    fn test_serialize_parameters() {
        let program_id = Pubkey::new_unique();
        let owner_pubkey = Pubkey::new_unique();
        let vault_pda = Pubkey::new_unique();
        let system_program = Pubkey::new_unique();

        let instruction = Instruction::new_with_bytes(
            program_id,
            &[1, 2, 3, 4], // instruction data
            vec![
                AccountMeta::new(owner_pubkey, true),
                AccountMeta::new(vault_pda, false),
                AccountMeta::new_readonly(system_program, false),
            ],
        );

        let accounts = vec![
            (
                owner_pubkey,
                SolAccount {
                    lamports: 10,
                    data: vec![1, 2, 3],
                    owner: Pubkey::new_unique(),
                    executable: false,
                    rent_epoch: 0,
                },
            ),
            (
                vault_pda,
                SolAccount {
                    lamports: 0,
                    data: vec![],
                    owner: Pubkey::new_unique(),
                    executable: false,
                    rent_epoch: 0,
                },
            ),
            (
                system_program,
                SolAccount {
                    lamports: 0,
                    data: vec![],
                    owner: Pubkey::new_unique(),
                    executable: false,
                    rent_epoch: 0,
                },
            ),
        ];

        let result = generate(&instruction, &accounts, "test_output.hex");
        assert!(result.is_ok());
    }

    #[test]
    fn test_serialize_parameters_with_duplicates() {
        let program_id = Pubkey::new_unique();
        let owner_pubkey = Pubkey::new_unique();
        let vault_pda = Pubkey::new_unique();

        let instruction = Instruction::new_with_bytes(
            program_id,
            &[1, 2, 3, 4], // instruction data
            vec![
                AccountMeta::new(owner_pubkey, true), // index 0
                AccountMeta::new(vault_pda, false),   // index 1
                AccountMeta::new(owner_pubkey, true), // duplicate of index 0
                AccountMeta::new(vault_pda, false),   // duplicate of index 1
            ],
        );

        let accounts = vec![
            (
                owner_pubkey,
                SolAccount {
                    lamports: 1,
                    data: vec![9, 9],
                    owner: Pubkey::new_unique(),
                    executable: false,
                    rent_epoch: 0,
                },
            ),
            (
                vault_pda,
                SolAccount {
                    lamports: 2,
                    data: vec![8, 8],
                    owner: Pubkey::new_unique(),
                    executable: false,
                    rent_epoch: 0,
                },
            ),
        ];

        let result = generate(&instruction, &accounts, "test_duplicates.hex");
        assert!(result.is_ok());
    }
}
