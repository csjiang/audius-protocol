#![cfg(feature = "test-bpf")]
mod utils;

use audius_reward_manager::{error::AudiusProgramError, instruction, processor::{TRANSFER_ACC_SPACE, TRANSFER_SEED_PREFIX, VERIFY_TRANSFER_SEED_PREFIX}, state::{VERIFIED_MESSAGES_LEN, VerifiedMessage}, utils::{find_derived_pair, EthereumAddress}, vote_message};
use libsecp256k1::{SecretKey};
use solana_program::{instruction::{Instruction}, program_pack::Pack, pubkey::Pubkey};
use solana_program_test::*;
use solana_sdk::{signature::Keypair, signer::Signer, transaction::{Transaction}};
use std::{mem::MaybeUninit};
use rand::{Rng};
use utils::*;


#[tokio::test]
/// Test a transfer can be completed successfully
async fn success_transfer() {
    let TestConstants { 
        reward_manager,
        bot_oracle_message,
        oracle_priv_key,
        senders_message,
        mut context,
        transfer_id,
        oracle_derived_address,
        recipient_eth_key,
        token_account,
        rent,
        mut rng,
        manager_account,
        recipient_sol_key,
        ..
    } = setup_test_environment().await;

    // Generate data and create senders
    let keys: [[u8; 32]; 3] = rng.gen();
    let operators: [EthereumAddress; 3] = rng.gen();
    let mut signers: [Pubkey; 3] = unsafe { MaybeUninit::zeroed().assume_init() };
    for (i, key) in keys.iter().enumerate() {
        let derived_address = create_sender_from(&reward_manager, &manager_account, &mut context, key, operators[i]).await;
        signers[i] = derived_address;
    }

    let mut instructions = Vec::<Instruction>::new();
    // Add 3 messages and bot oracle
    let oracle_sign =
        new_secp256k1_instruction_2_0(&oracle_priv_key, bot_oracle_message.as_ref(), 0);
    instructions.push(oracle_sign);

    for item in keys.iter().enumerate() {
        let priv_key = SecretKey::parse(item.1).unwrap();
        let inst = new_secp256k1_instruction_2_0(
            &priv_key,
            senders_message.as_ref(),
            (2 * item.0 + 1) as u8,
        );
        instructions.push(inst);
        instructions.push(
            instruction::submit_attestations(
                &audius_reward_manager::id(),
                &reward_manager.pubkey(),
                &signers[item.0],
                &context.payer.pubkey(),
                transfer_id.to_string()
            )
            .unwrap(),
        );
    }

    let oracle_sign = new_secp256k1_instruction_2_0(
        &oracle_priv_key,
        bot_oracle_message.as_ref(),
        (keys.len() * 2 + 1) as u8,
    );
    instructions.push(oracle_sign);
    instructions.push(
        instruction::submit_attestations(
            &audius_reward_manager::id(),
            &reward_manager.pubkey(),
            &oracle_derived_address,
            &context.payer.pubkey(),
            transfer_id.to_string()
        )
        .unwrap(),
    );

    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );

    context.banks_client.process_transaction(tx).await.unwrap();


    let transfer_account = get_transfer_account(&reward_manager, transfer_id);
    let verified_messages_account = get_messages_account(&reward_manager, transfer_id);
    let verified_messages_data = get_account(&mut context, &verified_messages_account).await.unwrap();
    assert_eq!(verified_messages_data.lamports, rent.minimum_balance(VERIFIED_MESSAGES_LEN));

    let tx = Transaction::new_signed_with_payer(
        &[instruction::evaluate_attestations(
            &audius_reward_manager::id(),
            &verified_messages_account,
            &reward_manager.pubkey(),
            &token_account.pubkey(),
            &recipient_sol_key.derive.address,
            &oracle_derived_address,
            &context.payer.pubkey(),
            10_000u64,
            transfer_id.to_string(),
            recipient_eth_key,
        )
        .unwrap()],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );

    context.banks_client.process_transaction(tx).await.unwrap();

    let transfer_account_data = get_account(&mut context, &transfer_account)
        .await
        .unwrap();

    assert_eq!(
        transfer_account_data.lamports,
        rent.minimum_balance(TRANSFER_ACC_SPACE)
    );
    assert_eq!(transfer_account_data.data.len(), TRANSFER_ACC_SPACE);

    // Assert that we transferred the expected amount
    let recipient_account_data = get_account(& mut context, &recipient_sol_key.derive.address).await.unwrap();
    let recipient_account = spl_token::state::Account::unpack(&recipient_account_data.data.as_slice()).unwrap();
    assert_eq!(recipient_account.amount, 10_000u64);

    // Assert that we wiped the verified messages account
    let verified_messages_data = get_account(&mut context, &verified_messages_account).await;
    assert!(verified_messages_data.is_none());
    
}

#[tokio::test]
/// Creates an invalid messages account by filling it wihout an oracle attestation,
/// validates that we see the expected error on calling `evaluate`, and then that we can
/// wipe the account by calling `submit` again with correct attestations and
/// finally succeed in `evaluate`.
async fn invalid_messages_are_wiped() {
    let TestConstants {
        reward_manager,
        bot_oracle_message,
        oracle_priv_key,
        senders_message,
        mut context,
        transfer_id,
        oracle_derived_address,
        mint,
        recipient_eth_key,
        token_account,
        mut rng,
        manager_account,
        ..
    } = setup_test_environment().await;

    // Generate data and create senders
    let keys: [[u8; 32]; 4] = rng.gen();
    let operators: [EthereumAddress; 4] = rng.gen();

    let mut signers: [Pubkey; 4] = unsafe { MaybeUninit::zeroed().assume_init() };
    for (i, key)in keys.iter().enumerate() {
        let derived_address = create_sender_from(&reward_manager, &manager_account, &mut context,key, operators[i]).await;
        signers[i] = derived_address;
    }

    let mut instructions = Vec::<Instruction>::new();

    // Add 4 DN attestations, no oracle
    for item in keys.iter().enumerate() {
        let priv_key = SecretKey::parse(item.1).unwrap();
        let inst = new_secp256k1_instruction_2_0(
            &priv_key,
            senders_message.as_ref(),
            (2 * item.0) as u8,
        );
        instructions.push(inst);
        instructions.push(
            instruction::submit_attestations(
                &audius_reward_manager::id(),
                &reward_manager.pubkey(),
                &signers[item.0],
                &context.payer.pubkey(),
                transfer_id.to_string()
            )
            .unwrap(),
        );
    }

    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );

    context.banks_client.process_transaction(tx).await.unwrap();

    let transfer_account = get_transfer_account(&reward_manager, transfer_id);
    let verified_messages_account = get_messages_account(&reward_manager, transfer_id);

    let recipient_sol_key = claimable_tokens::utils::program::find_address_pair(
        &claimable_tokens::id(),
        &mint.pubkey(),
        recipient_eth_key,
    )
    .unwrap();
    create_recipient_with_claimable_program(&mut context, &mint.pubkey(), recipient_eth_key).await;

    // attempt to evaluate the bad attestations
    let tx = Transaction::new_signed_with_payer(
        &[instruction::evaluate_attestations(
            &audius_reward_manager::id(),
            &verified_messages_account,
            &reward_manager.pubkey(),
            &token_account.pubkey(),
            &recipient_sol_key.derive.address,
            &oracle_derived_address,
            &context.payer.pubkey(),
            10_000u64,
            transfer_id.to_string(),
            recipient_eth_key,
        )
        .unwrap()],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );

    let res = context.banks_client.process_transaction(tx).await;
    // Ensure we got the expected NotEnoughSigners error
    assert_custom_error(res, 0, AudiusProgramError::NotEnoughSigners);

    // Ensure that the transfer account wasn't created
    let transfer_account_data = get_account(&mut context, &transfer_account)
        .await;
    assert!(transfer_account_data.is_none());

    // Try again to submit, this time with correct instructions
    let mut instructions = Vec::<Instruction>::new();

    // Only use operators 1-3
    for item in keys[..3].iter().enumerate() {
        let priv_key = SecretKey::parse(item.1).unwrap();
        let inst = new_secp256k1_instruction_2_0(
            &priv_key,
            senders_message.as_ref(),
            (2 * item.0) as u8,
        );
        instructions.push(inst);
        instructions.push(
            instruction::submit_attestations(
                &audius_reward_manager::id(),
                &reward_manager.pubkey(),
                &signers[item.0],
                &context.payer.pubkey(),
                transfer_id.to_string()
            )
            .unwrap(),
        );
    }
    // Now add the oracle
    let oracle_sign = new_secp256k1_instruction_2_0(
        &oracle_priv_key,
        bot_oracle_message.as_ref(),
        (instructions.len()) as u8,
    );
    instructions.push(oracle_sign);
    instructions.push(
        instruction::submit_attestations(
            &audius_reward_manager::id(),
            &reward_manager.pubkey(),
            &oracle_derived_address,
            &context.payer.pubkey(),
            transfer_id.to_string()
        )
        .unwrap(),
    );
    println!("{:?}", instructions.len());

    // Send the submit instruction
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );

    context.banks_client.process_transaction(tx).await.unwrap();

    // Send the evaluate instruction
    let recent_blockhash = context.banks_client.get_recent_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[instruction::evaluate_attestations(
            &audius_reward_manager::id(),
            &verified_messages_account,
            &reward_manager.pubkey(),
            &token_account.pubkey(),
            &recipient_sol_key.derive.address,
            &oracle_derived_address,
            &context.payer.pubkey(),
            10_000u64,
            transfer_id.to_string(),
            recipient_eth_key,
        )
        .unwrap()],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        recent_blockhash
    );

    context.banks_client.process_transaction(tx).await.unwrap();

    // Ensure we created the transfer account
    let transfer_account_data = get_account(&mut context, &transfer_account)
        .await;
    assert!(transfer_account_data.is_some());

    let recipient_account_data = get_account(& mut context, &recipient_sol_key.derive.address).await.unwrap();
    let recipient_account = spl_token::state::Account::unpack(&recipient_account_data.data.as_slice()).unwrap();
    assert_eq!(recipient_account.amount, 10_000u64)
}


#[tokio::test]
/// Purposefully send a corrupted DN attestation
async fn failure_transfer_invalid_message_format() {
    let TestConstants {
        reward_manager,
        bot_oracle_message,
        oracle_priv_key,
        mut context,
        transfer_id,
        oracle_derived_address,
        mint,
        recipient_eth_key,
        token_account,
        mut rng,
        manager_account,
        tokens_amount,
        eth_oracle_address,
        ..
     } = setup_test_environment().await;

    // Use invalid message format
    let senders_message = vote_message!([
        recipient_eth_key.as_ref(),
        b":",
        tokens_amount.to_le_bytes().as_ref(),
        b"_",
        transfer_id.as_ref(),
        b"_",
        eth_oracle_address.as_ref(),
    ]
    .concat());

    // Generate data and create senders
    let keys: [[u8; 32]; 3] = rng.gen();
    let operators: [EthereumAddress; 3] = rng.gen();
    let mut signers: [Pubkey; 3] = unsafe { MaybeUninit::zeroed().assume_init() };
    for (i, key) in keys.iter().enumerate() {
        let derived_address = create_sender_from(&reward_manager, &manager_account, &mut context, key, operators[i]).await;
        signers[i] = derived_address;
    }

    let mut instructions = Vec::<Instruction>::new();
    // Add 3 messages and bot oracle
    let oracle_sign =
        new_secp256k1_instruction_2_0(&oracle_priv_key, bot_oracle_message.as_ref(), 0);
    instructions.push(oracle_sign);

    for item in keys.iter().enumerate() {
        let priv_key = SecretKey::parse(item.1).unwrap();
        let inst = new_secp256k1_instruction_2_0(
            &priv_key,
            senders_message.as_ref(),
            (2 * item.0 + 1) as u8,
        );
        instructions.push(inst);
        instructions.push(
            instruction::submit_attestations(
                &audius_reward_manager::id(),
                &reward_manager.pubkey(),
                &signers[item.0],
                &context.payer.pubkey(),
                transfer_id.to_string()
            )
            .unwrap(),
        );
    }

    let oracle_sign = new_secp256k1_instruction_2_0(
        &oracle_priv_key,
        bot_oracle_message.as_ref(),
        (keys.len() * 2 + 1) as u8,
    );
    instructions.push(oracle_sign);
    instructions.push(
        instruction::submit_attestations(
            &audius_reward_manager::id(),
            &reward_manager.pubkey(),
            &oracle_derived_address,
            &context.payer.pubkey(),
            transfer_id.to_string()
        )
        .unwrap(),
    );

    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );

    context.banks_client.process_transaction(tx).await.unwrap();

    let verified_messages_derived_address = get_messages_account(&reward_manager, transfer_id);

    let recipient_sol_key = claimable_tokens::utils::program::find_address_pair(
        &claimable_tokens::id(),
        &mint.pubkey(),
        recipient_eth_key,
    )
    .unwrap();
    println!("Creating...Recipient sol key = {:?}", &recipient_sol_key.derive.address);
    create_recipient_with_claimable_program(&mut context, &mint.pubkey(), recipient_eth_key).await;
    println!("Created recipient sol key = {:?}", &recipient_sol_key.derive.address);

    let tx = Transaction::new_signed_with_payer(
        &[instruction::evaluate_attestations(
            &audius_reward_manager::id(),
            &verified_messages_derived_address,
            &reward_manager.pubkey(),
            &token_account.pubkey(),
            &recipient_sol_key.derive.address,
            &oracle_derived_address,
            &context.payer.pubkey(),
            10_000u64,
            transfer_id.to_string(),
            recipient_eth_key,
        )
        .unwrap()],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );

    let tx_result = context.banks_client.process_transaction(tx).await;
    assert_custom_error(tx_result, 0, AudiusProgramError::IncorrectMessages)
}

#[tokio::test]
/// Purposefully send a corrupted AAO Attestation
async fn failure_transfer_invalid_oracle_message_format() {

    let TestConstants {
        reward_manager,
        oracle_priv_key,
        senders_message,
        mut context,
        transfer_id,
        oracle_derived_address,
        recipient_eth_key,
        token_account,
        mut rng,
        manager_account,
        tokens_amount,
        recipient_sol_key,
        ..
    } = setup_test_environment().await;

    let bot_oracle_message = vote_message!([
        recipient_eth_key.as_ref(),
        b"|",
        tokens_amount.to_le_bytes().as_ref(),
        b"_",
        transfer_id.as_ref(),
    ]
    .concat());

    // Generate data and create senders
    let keys: [[u8; 32]; 3] = rng.gen();
    let operators: [EthereumAddress; 3] = rng.gen();
    let mut signers: [Pubkey; 3] = unsafe { MaybeUninit::zeroed().assume_init() };
    for (i, key) in keys.iter().enumerate() {
        let derived_address = create_sender_from(&reward_manager, &manager_account, &mut context, key, operators[i]).await;
        signers[i] = derived_address;
    }

    let mut instructions = Vec::<Instruction>::new();
    // Add 3 messages and bot oracle
    let oracle_sign =
        new_secp256k1_instruction_2_0(&oracle_priv_key, bot_oracle_message.as_ref(), 0);
    instructions.push(oracle_sign);

    for item in keys.iter().enumerate() {
        let priv_key = SecretKey::parse(item.1).unwrap();
        let inst = new_secp256k1_instruction_2_0(
            &priv_key,
            senders_message.as_ref(),
            (2 * item.0 + 1) as u8,
        );
        instructions.push(inst);
        instructions.push(
            instruction::submit_attestations(
                &audius_reward_manager::id(),
                &reward_manager.pubkey(),
                &signers[item.0],
                &context.payer.pubkey(),
                transfer_id.to_string()
            )
            .unwrap(),
        );
    }

    let oracle_sign = new_secp256k1_instruction_2_0(
        &oracle_priv_key,
        bot_oracle_message.as_ref(),
        (keys.len() * 2 + 1) as u8,
    );
    instructions.push(oracle_sign);
    instructions.push(
        instruction::submit_attestations(
            &audius_reward_manager::id(),
            &reward_manager.pubkey(),
            &oracle_derived_address,
            &context.payer.pubkey(),
            transfer_id.to_string()
        )
        .unwrap(),
    );

    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );

    context.banks_client.process_transaction(tx).await.unwrap();

    let verified_messages_derived_address = get_messages_account(&reward_manager, transfer_id);

    let tx = Transaction::new_signed_with_payer(
        &[instruction::evaluate_attestations(
            &audius_reward_manager::id(),
            &verified_messages_derived_address,
            &reward_manager.pubkey(),
            &token_account.pubkey(),
            &recipient_sol_key.derive.address,
            &oracle_derived_address,
            &context.payer.pubkey(),
            10_000u64,
            transfer_id.to_string(),
            recipient_eth_key,
        )
        .unwrap()],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );

    let tx_result = context.banks_client.process_transaction(tx).await;
    assert_custom_error(tx_result, 0, AudiusProgramError::IncorrectMessages)
}

#[tokio::test]
/// Test that we fail if missing AAO attestation
async fn failure_transfer_incorrect_number_of_verified_messages() {
    let TestConstants {
        reward_manager,
        bot_oracle_message,
        oracle_priv_key,
        senders_message,
        mut context,
        transfer_id,
        oracle_derived_address,
        mut rng,
        manager_account,
        ..
     } = setup_test_environment().await;

    // Generate data and create senders
    let keys: [[u8; 32]; 3] = rng.gen();
    let operators: [EthereumAddress; 3] = rng.gen();
    let mut signers: [Pubkey; 3] = unsafe { MaybeUninit::zeroed().assume_init() };
    for (i, key) in keys.iter().enumerate() {
        let derived_address = create_sender_from(&reward_manager, &manager_account, &mut context, key, operators[i]).await;
        signers[i] = derived_address;
    }

    let mut instructions = Vec::<Instruction>::new();
    // Add 3 messages and bot oracle
    let oracle_sign =
        new_secp256k1_instruction_2_0(&oracle_priv_key, bot_oracle_message.as_ref(), 0);
    instructions.push(oracle_sign);

    for item in keys.iter().enumerate() {
        let priv_key = SecretKey::parse(item.1).unwrap();
        let inst = new_secp256k1_instruction_2_0(
            &priv_key,
            senders_message.as_ref(),
            (2 * item.0 + 1) as u8,
        );
        instructions.push(inst);
        instructions.push(
            instruction::submit_attestations(
                &audius_reward_manager::id(),
                &reward_manager.pubkey(),
                &signers[item.0],
                &context.payer.pubkey(),
                transfer_id.to_string()
            )
            .unwrap(),
        );
    }

    instructions.push(
        instruction::submit_attestations(
            &audius_reward_manager::id(),
            &reward_manager.pubkey(),
            &oracle_derived_address,
            &context.payer.pubkey(),
            transfer_id.to_string()
        )
        .unwrap(),
    );

    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );

    // intentionally not push oracle instruction above and process transaction
    let tx_result = context.banks_client.process_transaction(tx).await;
    assert_custom_error(tx_result, 7, AudiusProgramError::Secp256InstructionMissing);
}

// Helpers

fn get_transfer_account(reward_manager: &Keypair, transfer_id: &str) -> Pubkey {
    let (_, transfer_derived_address, _) = find_derived_pair(
        &audius_reward_manager::id(),
        &reward_manager.pubkey(),
        [
            TRANSFER_SEED_PREFIX.as_bytes().as_ref(),
            transfer_id.as_ref(),
        ]
        .concat()
        .as_ref(),
    );
    transfer_derived_address
}

fn get_messages_account(reward_manager: &Keypair, transfer_id: &str) -> Pubkey {
    let (_, verified_messages_derived_address, _) = find_derived_pair(
        &audius_reward_manager::id(),
        &reward_manager.pubkey(),
        [
            VERIFY_TRANSFER_SEED_PREFIX.as_bytes().as_ref(),
            transfer_id.as_ref(),
        ]
        .concat()
        .as_ref(),
    );
    verified_messages_derived_address
}
