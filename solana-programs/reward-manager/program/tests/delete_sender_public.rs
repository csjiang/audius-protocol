#![cfg(feature = "test-bpf")]
mod utils;
use audius_reward_manager::{
    instruction,
    processor::SENDER_SEED_PREFIX,
    state::DELETE_SENDER_MESSAGE_PREFIX,
    utils::{find_derived_pair, EthereumAddress},
};
use libsecp256k1::{PublicKey, SecretKey};
use rand::{thread_rng, Rng};
use solana_program::program_pack::Pack;
use solana_program::{instruction::Instruction, pubkey::Pubkey};
use solana_program_test::*;
use solana_sdk::{
    secp256k1_instruction::construct_eth_pubkey, signature::Keypair, signer::Signer,
    transaction::Transaction,
};
use std::mem::MaybeUninit;
use utils::*;

#[tokio::test]
async fn success_delete_sender_public() {
    let program_test = program_test();
    let mut rng = thread_rng();

    let mint = Keypair::new();
    let mint_authority = Keypair::new();
    let token_account = Keypair::new();

    let reward_manager = Keypair::new();
    let manager_account = Keypair::new();
    let refunder_account = Pubkey::new_unique();
    let keys: [[u8; 32]; 4] = rng.gen();
    let mut signers: [Pubkey; 4] = unsafe { MaybeUninit::zeroed().assume_init() };

    for item in keys.iter().enumerate() {
        let sender_priv_key = SecretKey::parse(item.1).unwrap();
        let secp_pubkey = PublicKey::from_secret_key(&sender_priv_key);
        let eth_address = construct_eth_pubkey(&secp_pubkey);

        let (_, derived_address, _) = find_derived_pair(
            &audius_reward_manager::id(),
            &reward_manager.pubkey(),
            [SENDER_SEED_PREFIX.as_ref(), eth_address.as_ref()]
                .concat()
                .as_ref(),
        );

        signers[item.0] = derived_address;
    }

    let mut context = program_test.start_with_context().await;
    let rent = context.banks_client.get_rent().await.unwrap();

    create_mint(
        &mut context,
        &mint,
        rent.minimum_balance(spl_token::state::Mint::LEN),
        &mint_authority.pubkey(),
    )
    .await
    .unwrap();

    init_reward_manager(
        &mut context,
        &reward_manager,
        &token_account,
        &mint.pubkey(),
        &manager_account.pubkey(),
        3,
    )
    .await;

    // Create senders
    for key in &keys {
        let sender_priv_key = SecretKey::parse(&key).unwrap();
        let secp_pubkey = libsecp256k1::PublicKey::from_secret_key(&sender_priv_key);
        let eth_address = construct_eth_pubkey(&secp_pubkey);
        let operator: EthereumAddress = rng.gen();
        create_sender(
            &mut context,
            &reward_manager.pubkey(),
            &manager_account,
            eth_address,
            operator,
        )
        .await;
    }

    let mut instructions = Vec::<Instruction>::new();

    // get eth_address of sender which will be deleted
    let sender_priv_key = SecretKey::parse(&keys[3]).unwrap();
    let secp_pubkey = libsecp256k1::PublicKey::from_secret_key(&sender_priv_key);
    let eth_address = construct_eth_pubkey(&secp_pubkey);

    // Insert signs instructions
    let message = [
        DELETE_SENDER_MESSAGE_PREFIX.as_ref(),
        reward_manager.pubkey().as_ref(),
        eth_address.as_ref(),
    ]
    .concat();
    for item in keys[..3].iter().enumerate() {
        let priv_key = SecretKey::parse(item.1).unwrap();
        let inst = new_secp256k1_instruction_2_0(&priv_key, message.as_ref(), item.0 as _);
        instructions.push(inst);
    }

    instructions.push(
        instruction::delete_sender_public(
            &audius_reward_manager::id(),
            &reward_manager.pubkey(),
            &refunder_account,
            eth_address,
            &signers[..3],
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

    let (_, sender_solana_key, _) = find_derived_pair(
        &audius_reward_manager::id(),
        &reward_manager.pubkey(),
        [SENDER_SEED_PREFIX.as_ref(), eth_address.as_ref()]
            .concat()
            .as_ref(),
    );

    let account = context
        .banks_client
        .get_account(sender_solana_key)
        .await
        .unwrap();
    assert!(account.is_none());
}

#[tokio::test]
async fn failure_delete_sender_public_mismatched_signature_to_pubkey() {
    let program_test = program_test();
    let mut rng = thread_rng();

    let mint = Keypair::new();
    let mint_authority = Keypair::new();
    let token_account = Keypair::new();

    let reward_manager = Keypair::new();
    let manager_account = Keypair::new();
    let refunder_account = Pubkey::new_unique();
    let keys: [[u8; 32]; 4] = rng.gen();
    let mut signers: [Pubkey; 4] = unsafe { MaybeUninit::zeroed().assume_init() };

    for item in keys.iter().enumerate() {
        let sender_priv_key = SecretKey::parse(item.1).unwrap();
        let secp_pubkey = PublicKey::from_secret_key(&sender_priv_key);
        let eth_address = construct_eth_pubkey(&secp_pubkey);

        let (_, derived_address, _) = find_derived_pair(
            &audius_reward_manager::id(),
            &reward_manager.pubkey(),
            [SENDER_SEED_PREFIX.as_ref(), eth_address.as_ref()]
                .concat()
                .as_ref(),
        );

        signers[item.0] = derived_address;
    }

    let mut context = program_test.start_with_context().await;
    let rent = context.banks_client.get_rent().await.unwrap();

    create_mint(
        &mut context,
        &mint,
        rent.minimum_balance(spl_token::state::Mint::LEN),
        &mint_authority.pubkey(),
    )
    .await
    .unwrap();

    init_reward_manager(
        &mut context,
        &reward_manager,
        &token_account,
        &mint.pubkey(),
        &manager_account.pubkey(),
        3,
    )
    .await;

    // Create senders
    for key in &keys {
        let sender_priv_key = SecretKey::parse(&key).unwrap();
        let secp_pubkey = libsecp256k1::PublicKey::from_secret_key(&sender_priv_key);
        let eth_address = construct_eth_pubkey(&secp_pubkey);
        let operator: EthereumAddress = rng.gen();
        create_sender(
            &mut context,
            &reward_manager.pubkey(),
            &manager_account,
            eth_address,
            operator,
        )
        .await;
    }

    let mut instructions = Vec::<Instruction>::new();

    // get eth_address of sender which will be deleted
    let sender_priv_key = SecretKey::parse(&keys[3]).unwrap();
    let secp_pubkey = libsecp256k1::PublicKey::from_secret_key(&sender_priv_key);
    let eth_address = construct_eth_pubkey(&secp_pubkey);

    // Insert signs instructions
    let message = [
        DELETE_SENDER_MESSAGE_PREFIX.as_ref(),
        reward_manager.pubkey().as_ref(),
        eth_address.as_ref(),
    ]
    .concat();
    for item in keys[..3].iter().enumerate() {
        let priv_key = SecretKey::parse(item.1).unwrap();
        let inst = new_secp256k1_instruction_2_0(&priv_key, message.as_ref(), item.0 as _);
        instructions.push(inst);
    }

    // random index to denote which signer to replace with new pubkey
    let random_index = rand::thread_rng().gen_range(0..3);
    let mut new_signers: [Pubkey; 3] = [Keypair::new().pubkey(); 3];
    new_signers[..3].clone_from_slice(&signers[..3]);
    new_signers[random_index] = Keypair::new().pubkey();
    instructions.push(
        instruction::delete_sender_public(
            &audius_reward_manager::id(),
            &reward_manager.pubkey(),
            &refunder_account,
            eth_address,
            // use new signers
            &new_signers
        )
        .unwrap(),
    );

    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    let tx_result = context.banks_client.process_transaction(tx).await;

    match tx_result {
        Err(e) if e.to_string() == "transport transaction error: Error processing Instruction 3: Failed to serialize or deserialize account data: Unkown" => return (),
        Err(_) => panic!("Returned incorrect error!"),
        Ok(_) => panic!("Incorrectly returned Ok!"),
    }
}
