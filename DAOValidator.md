# How to run a NEAR Validator Stake&Farm contract controlled by SputnikDAO2


## Required tools

You need:

  1. GIT: https://git-scm.com/
  2. NEAR CLI: https://github.com/near/near-cli#Installation
  3. Go: https://go.dev/doc/install
  4. Install the nearkey tool:\
  `$ go install github.com/aurora-is-near/near-api-go/tools/cmd/nearkey@tools`\
  5. Rust (only if you are deploying the whitelist and factory contracts):\
  `$ curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`\
  `$ rustup target add wasm32-unknown-unknown`

## Create a DAO with Sputnik2

First, we have to create our Sputnik-DAO 2 DAO that will control the validator stake&farm contract.

  1. You need a NEAR account with sufficient (>200 NEAR) funds.
  2. Make the name of the account available for shell scripting:\
  `$ export MASTER=owner.testnet`
  3. Create the DAO factory. Download the Sputnik Repository:\
  `$ git clone https://github.com/near-daos/sputnik-dao-contract.git`
  4. Navigate to the source of the DAO2 factory:\
  `$ cd sputnik-dao-contract/sputnikdao-factory2`
  5. Build the factory contract:\
  `$ ./build.sh`
  6. Create the factory account on NEAR:\
  `$ near --masterAccount ${MASTER} create-account dao2factory.${MASTER}`
  7. Initialize the factory:\
  `$ near dao2factory.${MASTER} new '{}' --accountId dao2factory.${MASTER}`
  8. You can now collect the NEAR accounts of the council that controls the DAO.\
  We will use voter1.testnet, voter2.testnet and voter3.testnet as example in this Howto.
  9. Encode the council members into a JSON array:\
  `$ export COUNCIL='["voter1.testnet", "voter2.testnet", "voter3.testnet", "'${MASTER}'"]'`
  10. Select a name for your DAO:\
  `$ export DAONAME="farmdao"`
  11. Encode the DAO configuration into a base64 encoded JSON structure:\
  `$ export ARGS=$(echo '{"config": {"name": "'${DAONAME}'", "symbol": "FDAO", "decimals": 24, "purpose": "Operate Farming Validator", "bond": "1000000000000000000000000", "metadata": ""}, "policy": '$COUNCIL'}' | base64 -w 0)`
  12. Call the DAO factory to create the DAO:\
  `$ near call dao2factory.${MASTER} create "{\"name\": \"'${DAONAME}'\", \"public_key\": null, \"args\": \"'${ARGS}'\"}"  --accountId ${MASTER} --amount 30 --gas 300000000000000`
  13. Export your DAO name:\
  `$ export DAO=${DAONAME}.dao2factory.${MASTER}`
  14. Verify that all went well by viewing the voting policy of the DAO:\
  `$ near view ${DAO} get_policy`

## Create the Stake&Farm contract

The Stake&Farm contract needs to be created by the DAO. It receives rewards from
the work of an attached validator node, and distributes these rewards to stakeholders.

  1. Export the name of the staking pool factory:\
  `$ export FACTORY=stakepoolfactory.testnet`
  2. Select a name for your validator and make it globally available:\
  `$ export VALIDATORNAME=validator`
  3. Create the validator keypair:\
  `$ nearkey ${VALIDATORNAME}.${FACTORY} > validator_key.json`\
  This creates a file "validator_key.json" that needs to be deployed to your nearcore node installation.
  4. Copy the public_key from the validator_key.json file and make it publicly available:\
  `$ export VALIDATORKEY="ed25519:eSNAthKiUM1kNFifPDCt6U83Abnak4dCRbhUeNGA9j7"`
  5. The staking contract will be created by the DAO. This requires creating a vote proposal of type "Function Call".\
  The function to call is the staking pool factory, and the necessary configuration parameters have to be present.\
  Let's encode them as a base64 encoded JSON struct:\
  `$ export ARGS=$(echo '{ "staking_pool_id":"'${VALIDATORNAME}''", "code_hash":"HxT6MrNC7cQh68CZeBxiBbePSD7rxDeqeQDeHQ8n5j2M", "owner_id": "'${DAO}'", "stake_public_key": "'${VALIDATORKEY}'","reward_fee_fraction": {"numerator": 10, "denominator": 100}}' | base64 -w 0)`\
  The code-hash is the base58 encoded sha256 hash of the staking contract to be created.
  6. Now create the voting proposal. Be aware that the encoded gas in the call MUST be lower than the MaxGas minus the cost of the VoteApproval call.\
  `$ near call ${DAO} add_proposal '{"proposal": {"description": "Create Stake&Farm Contract", "submission_time":"600000000000", "kind": {"FunctionCall": {"receiver_id": "'${FACTORY}'", "actions":[{"method_name" : "create_staking_pool","deposit" : "30000000000000000000000000","gas": "150000000000000","args" : "'$ARGS'"}]}}}}' --accountId ${MASTER} --amount 1`
  7. Verify the proposal:\
  `$ near view ${DAO} get_proposals '{"from_index":0,"limit":10}'`\
  Each voting proposal returned contains an "id", which is required for the vote calls.\
  `$ export VOTEID=0`
  8. Vote to accept the proposal:\
  `$ near call ${DAO} act_proposal '{"id": '${VOTEID}', "action": "VoteApprove"}' --gas 300000000000000 --accountId  ${MASTER}`\
  Make sure that the maximum allowed gas is attached. As soon as enough voters approved the proposal, it will be executed.
  9. Verify result:\
  `$ near state ${VALIDATORNAME}.${FACTORY}`
  10. The new staking contract can now only be controlled by the DAO, which in turn can only be controlled by the council.

## Configure a token farm for the Stake&Farm contract.

Here we assume that an fungible token contract already exists and that ${MASTER} owns sufficient tokens in it already.

  1. Export the name of the token contract:\
  `$ export TOKEN=token123.testnet`
  2. Create storage in the token contract so that the DAO can hold the token:\
  `$ near call ${TOKEN} storage_deposit '{"account_id": "'${DAO}'"}' --accountId ${OWNER} --amount 0.00125`
  3. Create storage in the token contract so that the staking contract can hold the token:\
  `$ near call ${TOKEN} storage_deposit '{"account_id": "'${VALIDATORNAME}.${FACTORY}'"}' --accountId ${OWNER} --amount 0.00125`
  4. Transfer tokens to the DAO:\
  `$ near call ${TOKEN} ft_transfer '{"receiver_id": "'${DAO}'", "amount": "1000000000000000"}' --accountId ${MASTER} --amount 0.000000000000000000000001`\
  Make sure to include the mandatory yoctoNEAR deposit.
  5. Now we have to configure the proposal to vote on creating the farm entry in the staking contract. This is implemented by a Function Call proposal again:\
  `$ export STARTIN=360` Start farming in so many seconds from now.\
  `$ export DURATION=3600` Farming continues for so many seconds.\
  `$ export STARTDATE=$(expr $(date +%s) + ${STARTIN})` Calculate start unixtime for the farm.\
  `$ export ENDDATE=$(expr ${STARTDATE} + ${DURATION})"000000000"` Calculate end unixtime nanoseconds for the farm.\
  `$ export STARTDATE=${STARTDATE}"000000000"` Turn STARTDATE into nanoseconds.
  6. We encode the proposal parameters as a base64 encoded JSON struct:\ 
  `$ export ARGS=$(echo '{ "receiver_id": "'${VALIDATORNAME}.${FACTORY}'", "amount": "1000000000000000", "msg": "{\"name\": \"Mega Token\", \"start_date\": \"'${STARTDATE}'\", \"end_date\": \"'${ENDDATE}'\" }" }' | base64 -w 0)`
  7. And finally enter the proposal with the DAO:\
  `$ near call $DAO add_proposal '{"proposal": {"description": "Create Stake&Farm Contract", "submission_time":"600000000000", "kind": {"FunctionCall": {"receiver_id": "'${TOKEN}'", "actions":[{"method_name" : "ft_transfer_call","deposit" : "1","gas": "150000000000000","args" : "'$ARGS'"}]}}}}' --accountId cowfaction.testnet --amount 1`\
  This means that when the proposal is accepted by enough council members, the DAO will call the Token contract to transfer tokens to the Staking contract with the attached configuration. This creates the farm.
  8. Verify after voting:\
  `$ near view ${VALIDATORNAME}.${FACTORY} get_farms '{"from_index":0,"limit":10}'`

## FaQ

[tbd]
