#[cfg(test)]
mod tests {
    use mollusk_svm::{program, result::Check, Mollusk};
    use solana_sdk::account::Account;
    use solana_sdk::instruction::{AccountMeta, Instruction};
    use solana_sdk::pubkey::Pubkey;

    #[test]
    fn test_sample() {
        let program_id_keypair_bytes = std::fs::read("deploy/sample-keypair.json").unwrap()[..32]
            .try_into()
            .expect("slice with incorrect length");
        let program_id = Pubkey::new_from_array(program_id_keypair_bytes);

        let (system_program, _) = program::keyed_account_for_system_program();

        let owner_pubkey = Pubkey::new_unique();
        let owner_account = Account::new(1000000000, 0, &system_program);

        let instruction = Instruction::new_with_bytes(
            program_id,
            &[93],
            vec![AccountMeta::new(owner_pubkey, true)],
        );

        // Generate debugger input.
        sbpf_dbg_input::generate(&instruction, "sample_input").unwrap();

        let mollusk = Mollusk::new(&program_id, "deploy/sample");

        let result = mollusk.process_and_validate_instruction(
            &instruction,
            &[(owner_pubkey, owner_account)],
            &[Check::success()],
        );
        assert!(!result.program_result.is_err());
    }
}
