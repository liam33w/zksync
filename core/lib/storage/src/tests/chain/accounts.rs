// External imports
// Workspace imports
use zksync_types::{aggregated_operations::AggregatedActionType, AccountMap};
// Local imports
use super::block::apply_random_updates;
use crate::tests::{create_rng, db_test};
use crate::{
    chain::{
        account::{records::EthAccountType, AccountSchema},
        block::BlockSchema,
        state::StateSchema,
    },
    test_data::gen_operation,
    QueryResult, StorageProcessor,
};

/// The save/load routine for EthAccountType
#[db_test]
async fn eth_account_type(mut storage: StorageProcessor<'_>) -> QueryResult<()> {
    // check that function returns None by default
    let non_existent = AccountSchema(&mut storage).account_type_by_id(18).await?;
    assert!(non_existent.is_none());

    // store account type and then load it
    AccountSchema(&mut storage)
        .set_account_type(18, EthAccountType::CREATE2)
        .await?;
    let loaded = AccountSchema(&mut storage).account_type_by_id(18).await?;
    assert!(matches!(loaded, Some(EthAccountType::CREATE2)));

    Ok(())
}

/// Checks that stored accounts can be obtained once they're committed.
#[db_test]
async fn stored_accounts(mut storage: StorageProcessor<'_>) -> QueryResult<()> {
    let _ = env_logger::try_init();
    let mut rng = create_rng();

    let block_size = 100;

    // Create several accounts.
    let (accounts_block, updates_block) = apply_random_updates(AccountMap::default(), &mut rng);

    // Execute and commit block with them.
    // Also store account updates.
    BlockSchema(&mut storage)
        .save_block(get_sample_block(1, block_size, Default::default()))
        .await?;
    StateSchema(&mut storage)
        .commit_state_update(1, &updates_block, 0)
        .await?;

    // Get the accounts by their addresses.
    for (account_id, account) in accounts_block.iter() {
        let mut account = account.clone();
        let account_state = AccountSchema(&mut storage)
            .account_state_by_address(account.address)
            .await?;

        // Check that committed state is available, but verified is not.
        assert!(
            account_state.committed.is_some(),
            "No committed state for account"
        );
        assert!(
            account_state.verified.is_none(),
            "Block is not verified, but account has a verified state"
        );

        // Compare the obtained stored account with expected one.
        let (got_account_id, got_account) = account_state.committed.unwrap();

        // We have to copy this field, since it is not initialized by default.
        account.pub_key_hash = got_account.pub_key_hash;

        assert_eq!(got_account_id, *account_id);
        assert_eq!(got_account, account);

        // Also check `last_committed_state_for_account` method.
        assert_eq!(
            AccountSchema(&mut storage)
                .last_committed_state_for_account(*account_id)
                .await?,
            Some(got_account)
        );

        // Check account address and ID getters.
        assert_eq!(
            AccountSchema(&mut storage)
                .account_address_by_id(*account_id)
                .await?,
            Some(account.address)
        );
        assert_eq!(
            AccountSchema(&mut storage)
                .account_id_by_address(account.address)
                .await?,
            Some(*account_id)
        );
    }

    // Now add a proof, verify block and apply a state update.
    OperationsSchema(&mut storage)
        .store_aggregated_action(gen_unique_aggregated_operation(
            1,
            AggregatedActionType::ExecuteBlocks,
            block_size,
        ))
        .await?;
    StateSchema(&mut storage).apply_state_update(1).await?;

    // After that all the accounts should have a verified state.
    for (account_id, account) in accounts_block {
        let account_state = AccountSchema(&mut storage)
            .account_state_by_id(account_id)
            .await?;

        assert!(
            account_state.committed.is_some(),
            "No committed state for account"
        );
        assert!(
            account_state.verified.is_some(),
            "No verified state for the account"
        );

        // Compare the obtained stored account with expected one.
        let (got_account_id, got_account) = account_state.verified.unwrap();

        assert_eq!(got_account_id, account_id);
        assert_eq!(got_account, account);

        // Also check `last_verified_state_for_account` method.
        assert_eq!(
            AccountSchema(&mut storage)
                .last_verified_state_for_account(account_id)
                .await?,
            Some(got_account)
        );
    }

    Ok(())
}
