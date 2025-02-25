use near_contract_standards::fungible_token::core_impl::ext_fungible_token;
use near_sdk::json_types::U64;
use near_sdk::{promise_result_as_success, Timestamp};

use crate::internal::ZERO_ADDRESS;
use crate::stake::ext_self;
use crate::*;

const SESSION_INTERVAL: u64 = 1_000_000_000;
const DENOMINATOR: u128 = 1_000_000_000_000_000_000_000_000;

/// Amount of gas for fungible token transfers.
pub const GAS_FOR_FT_TRANSFER: Gas = Gas(10_000_000_000_000);
/// hotfix_insuffient_gas_for_mft_resolve_transfer, increase from 5T to 20T
pub const GAS_FOR_RESOLVE_TRANSFER: Gas = Gas(20_000_000_000_000);
/// Gas for calling `get_owner` method.
pub const GAS_FOR_GET_OWNER: Gas = Gas(10_000_000_000_000);
pub const GAS_LEFTOVERS: Gas = Gas(20_000_000_000_000);
/// Get owner method on external contracts.
pub const GET_OWNER_METHOD: &str = "get_owner_account_id";

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub struct RewardDistribution {
    pub undistributed: Balance,
    pub unclaimed: Balance,
    pub rps: U256,
    pub reward_round: u64,
}

#[derive(BorshSerialize, BorshDeserialize)]
pub struct Farm {
    name: String,
    token_id: AccountId,
    amount: Balance,
    start_date: Timestamp,
    end_date: Timestamp,
    last_distribution: RewardDistribution,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(crate = "near_sdk::serde")]
pub struct HumanReadableFarm {
    pub name: String,
    pub token_id: AccountId,
    pub amount: U128,
    pub start_date: U64,
    pub end_date: U64,
}

impl From<Farm> for HumanReadableFarm {
    fn from(farm: Farm) -> Self {
        HumanReadableFarm {
            name: farm.name,
            token_id: farm.token_id,
            amount: U128(farm.amount),
            start_date: U64(farm.start_date),
            end_date: U64(farm.end_date),
        }
    }
}

impl StakingContract {
    pub(crate) fn internal_deposit_farm_tokens(
        &mut self,
        token_id: &AccountId,
        name: String,
        amount: Balance,
        start_date: Timestamp,
        end_date: Timestamp,
    ) {
        assert!(start_date >= env::block_timestamp(), "ERR_FARM_TOO_EARLY");
        assert!(end_date > start_date, "ERR_FARM_DATE");
        assert!(amount > 0, "ERR_FARM_AMOUNT_NON_ZERO");
        assert!(
            amount / ((end_date - start_date) / SESSION_INTERVAL) as u128 > 0,
            "ERR_FARM_AMOUNT_TOO_SMALL"
        );
        self.farms.push(&Farm {
            name,
            token_id: token_id.clone(),
            amount,
            start_date,
            end_date,
            last_distribution: RewardDistribution {
                undistributed: amount,
                unclaimed: 0,
                rps: U256::zero(),
                reward_round: 0,
            },
        });
    }

    fn internal_get_farm(&self, farm_id: u64) -> Farm {
        self.farms.get(farm_id).expect("ERR_NO_FARM")
    }

    fn internal_calculate_distribution(
        &self,
        farm: &Farm,
        total_staked: Balance,
    ) -> Option<RewardDistribution> {
        if farm.start_date > env::block_timestamp() {
            // Farm hasn't started.
            return None;
        }
        let mut distribution = farm.last_distribution.clone();
        if distribution.undistributed == 0 {
            // Farm has ended.
            return Some(distribution);
        }
        distribution.reward_round = (env::block_timestamp() - farm.start_date) / SESSION_INTERVAL;
        let reward_per_session =
            farm.amount / (farm.end_date - farm.start_date) as u128 * SESSION_INTERVAL as u128;
        let mut reward_added = (distribution.reward_round - farm.last_distribution.reward_round)
            as u128
            * reward_per_session;
        if farm.last_distribution.undistributed < reward_added {
            // Last step when the last tokens are getting distributed.
            reward_added = farm.last_distribution.undistributed;
            let increase_reward_round = (reward_added / reward_per_session) as u64;
            distribution.reward_round = farm.last_distribution.reward_round + increase_reward_round;
            if increase_reward_round as u128 * reward_per_session < reward_added {
                // Fix the rounding.
                distribution.reward_round += 1;
            }
        }
        distribution.unclaimed += reward_added;
        distribution.undistributed -= reward_added;
        if total_staked == 0 {
            distribution.rps = U256::zero();
        } else {
            distribution.rps = farm.last_distribution.rps
                + U256::from(reward_added) * U256::from(DENOMINATOR) / U256::from(total_staked);
        }
        Some(distribution)
    }

    fn internal_unclaimed_balance(
        &self,
        account: &Account,
        farm_id: u64,
        farm: &Farm,
    ) -> (U256, Balance) {
        let user_rps = account
            .user_rps
            .get(&farm_id)
            .cloned()
            .unwrap_or(U256::zero());
        if let Some(distribution) = self.internal_calculate_distribution(
            &farm,
            self.total_stake_shares - self.total_burn_shares,
        ) {
            (
                farm.last_distribution.rps,
                (U256::from(account.stake_shares) * (distribution.rps - user_rps) / DENOMINATOR)
                    .as_u128(),
            )
        } else {
            (U256::zero(), 0)
        }
    }

    fn internal_distribute(&mut self, farm: &mut Farm) {
        if let Some(distribution) = self.internal_calculate_distribution(
            &farm,
            self.total_stake_shares - self.total_burn_shares,
        ) {
            if distribution.reward_round != farm.last_distribution.reward_round {
                farm.last_distribution = distribution;
            }
        }
    }

    fn internal_distribute_reward(
        &mut self,
        account: &mut Account,
        farm_id: u64,
        mut farm: &mut Farm,
    ) {
        self.internal_distribute(&mut farm);
        let (new_user_rps, claim_amount) =
            self.internal_unclaimed_balance(&account, farm_id, &farm);
        account.user_rps.insert(farm_id, new_user_rps);
        *account.amounts.entry(farm.token_id.clone()).or_default() += claim_amount;
    }

    /// Distribute all rewards for the given user.
    pub(crate) fn internal_distribute_all_rewards(&mut self, mut account: &mut Account) {
        for farm_id in 0..self.farms.len() {
            if let Some(mut farm) = self.farms.get(farm_id) {
                self.internal_distribute_reward(&mut account, farm_id, &mut farm);
                self.farms.replace(farm_id, &farm);
            }
        }
    }

    fn internal_user_token_deposit(
        &mut self,
        account_id: &AccountId,
        token_id: &AccountId,
        amount: Balance,
    ) {
        let mut account = self.accounts.get(&account_id).expect("ERR_NO_ACCOUNT");
        *account.amounts.entry(token_id.clone()).or_default() += amount;
        self.accounts.insert(&account_id, &account);
    }

    fn internal_claim(
        &mut self,
        token_id: &AccountId,
        claim_account_id: &AccountId,
        send_account_id: &AccountId,
    ) -> Promise {
        let mut account = self
            .accounts
            .get(&claim_account_id)
            .expect("ERR_NO_ACCOUNT");
        self.internal_distribute_all_rewards(&mut account);
        let amount = account.amounts.remove(&token_id).unwrap_or(0);
        self.accounts.insert(&claim_account_id, &account);
        ext_fungible_token::ft_transfer(
            send_account_id.clone(),
            U128(amount),
            None,
            token_id.clone(),
            1,
            GAS_FOR_FT_TRANSFER,
        )
        .then(ext_self::callback_post_withdraw_reward(
            token_id.clone(),
            send_account_id.clone(),
            U128(amount),
            env::current_account_id(),
            0,
            GAS_FOR_RESOLVE_TRANSFER,
        ))
    }
}

#[near_bindgen]
impl StakingContract {
    /// Callback after checking owner for the delegated claim.
    #[private]
    pub fn callback_post_get_owner(
        &mut self,
        token_id: AccountId,
        delegator_id: AccountId,
        account_id: AccountId,
    ) -> Promise {
        let owner_id: AccountId = near_sdk::serde_json::from_slice(
            &promise_result_as_success().expect("get_owner must have result"),
        )
        .expect("Failed to parse");
        assert_eq!(owner_id, account_id, "Caller is not an owner");
        self.internal_claim(&token_id, &delegator_id, &account_id)
    }

    /// Callback from depositing funds to the user's account.
    /// If it failed, return funds to the user's account.
    #[private]
    pub fn callback_post_withdraw_reward(
        &mut self,
        token_id: AccountId,
        sender_id: AccountId,
        amount: U128,
    ) {
        if promise_result_as_success().is_none() {
            // This reverts the changes from the claim function.
            self.internal_user_token_deposit(&sender_id, &token_id, amount.0);
        }
    }

    /// Claim given tokens for given account.
    /// If delegator is provided, it will call it's `get_owner` method to confirm that caller
    /// can execute on behalf of this contract.
    pub fn claim(&mut self, token_id: AccountId, delegator_id: Option<AccountId>) -> Promise {
        let account_id = env::predecessor_account_id();
        if let Some(delegator_id) = delegator_id {
            Promise::new(delegator_id.clone())
                .function_call(GET_OWNER_METHOD.to_string(), vec![], 0, GAS_FOR_GET_OWNER)
                .then(ext_self::callback_post_get_owner(
                    token_id,
                    delegator_id,
                    account_id,
                    env::current_account_id(),
                    0,
                    env::prepaid_gas() - env::used_gas() - GAS_FOR_GET_OWNER - GAS_LEFTOVERS,
                ))
        } else {
            self.internal_claim(&token_id, &account_id, &account_id)
        }
    }

    pub fn get_farms(&self, from_index: u64, limit: u64) -> Vec<HumanReadableFarm> {
        (from_index..std::cmp::min(from_index + limit, self.farms.len()))
            .map(|index| self.farms.get(index).unwrap().into())
            .collect()
    }

    pub fn get_farm(&self, farm_id: u64) -> HumanReadableFarm {
        self.internal_get_farm(farm_id).into()
    }

    pub fn get_unclaimed_reward(&self, account_id: AccountId, farm_id: u64) -> U128 {
        if account_id == AccountId::new_unchecked(ZERO_ADDRESS.to_string()) {
            return U128(0);
        }
        let account = self.accounts.get(&account_id).expect("ERR_NO_ACCOUNT");
        let farm = self.farms.get(farm_id).expect("ERR_NO_FARM");
        let (_rps, reward) = self.internal_unclaimed_balance(&account, farm_id, &farm);
        let prev_reward = *account.amounts.get(&farm.token_id).unwrap_or(&0);
        U128(reward + prev_reward)
    }
}
