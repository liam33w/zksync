use std::iter;

use num::BigUint;
use rand::{seq::SliceRandom, Rng};

use zksync_types::Address;

use crate::{account_pool::AddressPool, rng::LoadtestRng};

/// Type of transaction. It doesn't copy the zkSync operation list, because
/// it divides some transactions in subcategories (e.g. to new account / to existing account; to self / to other; etc)/
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum TxType {
    Deposit,
    TransferToNew,
    TransferToExisting,
    WithdrawToSelf,
    WithdrawToOther,
    FullExit,
    ChangePubKey,
}

impl TxType {
    /// Generates a random transaction type. Not all the variants have the equal chance to be generated;
    /// specifically transfers are made more likely.
    pub fn random(rng: &mut LoadtestRng) -> Self {
        // All available options.
        let mut options = vec![
            Self::Deposit,
            Self::TransferToNew,
            Self::TransferToExisting,
            Self::WithdrawToSelf,
            Self::WithdrawToOther,
            Self::FullExit,
            Self::ChangePubKey,
        ];

        // Make `TransferToNew` and `TransferToExisting` the most likely options
        // by adding them multiple times.
        let transfer_to_new_likehood = 0.3f64;
        let transfer_to_existing_likehood = 0.4f64;

        // We are ignoring the fact that variables in fact rely on each other; it's not that important for our purposes.
        let required_transfer_to_new_copies =
            Self::required_amount_of_copies(&options, transfer_to_new_likehood);
        let required_transfer_to_existing_copies =
            Self::required_amount_of_copies(&options, transfer_to_existing_likehood);
        let total_new_elements =
            required_transfer_to_new_copies + required_transfer_to_existing_copies;

        options.reserve(total_new_elements);

        options.extend(iter::repeat(Self::TransferToNew).take(required_transfer_to_new_copies));
        options.extend(
            iter::repeat(Self::TransferToExisting).take(required_transfer_to_existing_copies),
        );

        // Now we can get weighted element by simply choosing the random value from the vector.
        options.choose(rng).copied().unwrap()
    }

    /// Generates a random transaction type that can be a part of the batch.
    pub fn random_batchable(rng: &mut LoadtestRng) -> Self {
        loop {
            let output = Self::random(rng);

            // Priority ops and ChangePubKey cannot be inserted into the batch.
            if !matches!(output, Self::Deposit | Self::FullExit | Self::ChangePubKey) {
                return output;
            }
        }
    }

    fn required_amount_of_copies(options: &[Self], likehood: f64) -> usize {
        // This value will be truncated down, but it will be compensated by the fact
        // that element is already inserted into `options`.
        (options.len() as f64 * likehood) as usize
    }
}

/// Modifier to be applied to the transaction in order to make it incorrect.
/// Incorrect transactions are a significant part of loadtest, because we want to ensure
/// that server is resilient for all the possible kinds of user input.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum IncorrectnessModifier {
    ZeroFee,
    IncorrectZkSyncSignature,
    IncorrectEthSignature,
    NonExistentToken,
    TooBigAmount,
    NotPackableAmount,
    NotPackableFeeAmount,

    // Last option goes for no modifier,
    // since it's more convenient than dealing with `Option<IncorrectnessModifier>`.
    None,
}

/// Expected outcome of transaction:
/// Since we may create erroneous transactions on purpose,
/// we may expect different outcomes for each transaction.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ExpectedOutcome {
    /// Transactions was successfully executed.
    TxSucceed,
    /// Transaction sending should fail.
    ApiRequestFailed,
    /// Transaction should be accepted, but rejected at the
    /// time of execution.
    TxRejected,
}

impl IncorrectnessModifier {
    pub fn random(rng: &mut LoadtestRng) -> Self {
        // 90% of transactions should be correct.
        const NO_MODIFIER_PROBABILITY: f32 = 0.9f32;
        // Amount of elements in the enum.
        const MODIFIERS_AMOUNT: usize = 7;

        let chance = rng.gen_range(0f32, 1f32);
        if chance <= NO_MODIFIER_PROBABILITY {
            return Self::None;
        }

        let modifier_type = rng.gen_range(0, MODIFIERS_AMOUNT);

        match modifier_type {
            0 => Self::ZeroFee,
            1 => Self::IncorrectZkSyncSignature,
            2 => Self::IncorrectEthSignature,
            3 => Self::NonExistentToken,
            4 => Self::TooBigAmount,
            5 => Self::NotPackableAmount,
            6 => Self::NotPackableFeeAmount,
            _ => unreachable!("Unexpected modifier type number"),
        }
    }

    pub fn expected_outcome(self) -> ExpectedOutcome {
        match self {
            Self::None => ExpectedOutcome::TxSucceed,

            Self::ZeroFee
            | Self::IncorrectEthSignature
            | Self::IncorrectZkSyncSignature
            | Self::NonExistentToken
            | Self::NotPackableAmount
            | Self::NotPackableFeeAmount => ExpectedOutcome::ApiRequestFailed,

            Self::TooBigAmount => ExpectedOutcome::TxRejected,
        }
    }
}

/// Complete description of a transaction that must be executed by a test wallet.
#[derive(Debug, Clone)]
pub struct TxCommand {
    /// Type of operation.
    pub command_type: TxType,
    /// Whether and how transaction should be corrupted.
    pub modifier: IncorrectnessModifier,
    /// Recipient address.
    pub to: Address,
    /// Transaction amount (0 if not applicable).
    pub amount: BigUint,
}

impl TxCommand {
    pub fn change_pubkey(address: Address) -> Self {
        Self {
            command_type: TxType::ChangePubKey,
            modifier: IncorrectnessModifier::None,
            to: address,
            amount: 0u64.into(),
        }
    }

    /// Generates a fully random transaction command.
    pub fn random(rng: &mut LoadtestRng, own_address: Address, addresses: &AddressPool) -> Self {
        let command_type = TxType::random(rng);

        Self::new_with_type(rng, own_address, addresses, command_type)
    }

    /// Generates a random transaction command that can be a part of the batch.
    pub fn random_batchable(
        rng: &mut LoadtestRng,
        own_address: Address,
        addresses: &AddressPool,
    ) -> Self {
        let command_type = TxType::random_batchable(rng);

        Self::new_with_type(rng, own_address, addresses, command_type)
    }

    fn new_with_type(
        rng: &mut LoadtestRng,
        own_address: Address,
        addresses: &AddressPool,
        command_type: TxType,
    ) -> Self {
        let mut command = Self {
            command_type,
            modifier: IncorrectnessModifier::random(rng),
            to: addresses.random_address(rng),
            amount: Self::random_amount(rng),
        };

        // Check whether we should use a non-existent address.
        if matches!(command.command_type, TxType::TransferToNew) {
            command.to = Address::random();
        }

        // Check whether we should use a self as an target.
        if matches!(
            command.command_type,
            TxType::WithdrawToSelf | TxType::FullExit
        ) {
            command.to = own_address;
        }

        // `ChangePubKey` does not have a 2FA signature.
        let cpk_incorrect_signature = command.command_type == TxType::ChangePubKey
            && command.modifier == IncorrectnessModifier::IncorrectEthSignature;
        // Transactions that have no amount field.
        let no_amount_field = matches!(command.command_type, TxType::ChangePubKey)
            && matches!(
                command.modifier,
                IncorrectnessModifier::TooBigAmount | IncorrectnessModifier::NotPackableAmount
            );
        // It doesn't make sense to fail contract-based functions.
        let incorrect_priority_op =
            matches!(command.command_type, TxType::Deposit | TxType::FullExit);
        // Amount doesn't have to be packable for withdrawals.
        let unpackable_withdrawal = matches!(
            command.command_type,
            TxType::WithdrawToOther | TxType::WithdrawToSelf
        ) && command.modifier
            == IncorrectnessModifier::NotPackableAmount;

        // Check whether generator modifier does not make sense.
        if cpk_incorrect_signature
            || no_amount_field
            || incorrect_priority_op
            || unpackable_withdrawal
        {
            command.modifier = IncorrectnessModifier::None;
        }

        command
    }

    fn random_amount(rng: &mut LoadtestRng) -> BigUint {
        rng.gen_range(0u64, 2u64.pow(18)).into()
    }
}
