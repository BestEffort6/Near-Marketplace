const nearAPI = require("near-api-js");
const {
  utils: {
    format: { parseNearAmount },
  },
} = nearAPI;
const getConfig = require("./config");
const {
  stakingContractName,
  gas,
  gas_max,
  nodeUrl,
  walletUrl,
  ownerAccountName,
  bettingContractName,
  networkId,
} = getConfig("testnet");

const keyStore1 = new nearAPI.keyStores.InMemoryKeyStore();
const PRIVATE_KEY1 =
  "ed25519:9nYNwsP7mYqMLRsouSLaqKCBZTFcs8R34CVWgHNqoP351VVqsRmdPvKax8XCqWcKnGNsy45AYuofw6UsMPEJfdE"; //vier1near
const keyPair1 = nearAPI.KeyPair.fromString(PRIVATE_KEY1);

keyStore1.setKey("testnet", "vier1near.testnet", keyPair1);

const near1 = new nearAPI.Near({
  deps: {
    keyStore: keyStore1,
  },
  networkId: networkId,
  keyStore: keyStore1,
  nodeUrl: nodeUrl,
  walletUrl: walletUrl,
});

const ownerAccount = new nearAPI.Account(near1.connection, ownerAccountName);

const keyStore2 = new nearAPI.keyStores.InMemoryKeyStore();
const PRIVATE_KEY2 =
  "ed25519:31Hvsifgw7kpN55sa3N6F8L6tA4Ge7XUaXtcc7nCCxTNA2MvBciR7MgC4fhypCyLN9PCnvPZtDA7UgizufCY6qNU"; //viernear
const keyPair2 = nearAPI.KeyPair.fromString(PRIVATE_KEY2);

keyStore2.setKey("testnet", "viernear.testnet", keyPair2);

const near2 = new nearAPI.Near({
  deps: {
    keyStore: keyStore2,
  },
  networkId: networkId,
  keyStore: keyStore2,
  nodeUrl: nodeUrl,
  walletUrl: walletUrl,
});

const userAccount = new nearAPI.Account(near2.connection, "viernear.testnet");

const stakingContract = new nearAPI.Contract(
  ownerAccount,
  stakingContractName,
  {
    viewMethods: [],
    changeMethods: [],
  }
);

module.exports = {
  gas,
  gas_max,
  stakingContract,
  ownerAccount,
  bettingContractName,
  userAccount,
};
