const assert = require("assert");
const testUtils = require("./test-utils");

const { stakingContract, gas, ownerAccount, userAccount } = testUtils;

const usdcAccount = "cusd.fakes.testnet";
const vexAccount = "vex.vex-betting.testnet";
const bettingContractName = "betting.vex-betting.testnet";
const refContractName = "ref-finance-101.testnet";

const storageDeposit = async () => {
  try {
    await ownerAccount.functionCall({
      contractId: vexAccount,
      methodName: "storage_deposit",
      args: {
        account_id: stakingContract.contractId,
      },
      gas: gas,
      attachedDeposit: "10000000000000000000000",
    });
    await ownerAccount.functionCall({
      contractId: usdcAccount,
      methodName: "storage_deposit",
      args: {
        account_id: stakingContract.contractId,
      },
      gas: gas,
      attachedDeposit: "10000000000000000000000",
    });
    await ownerAccount.functionCall({
      contractId: vexAccount,
      methodName: "storage_deposit",
      args: {
        account_id: userAccount.accountId,
      },
      gas: gas,
      attachedDeposit: "10000000000000000000000",
    });
    await ownerAccount.functionCall({
      contractId: usdcAccount,
      methodName: "storage_deposit",
      args: {
        account_id: userAccount.accountId,
      },
      gas: gas,
      attachedDeposit: "10000000000000000000000",
    });
    await ownerAccount.functionCall({
      contractId: usdcAccount,
      methodName: "storage_deposit",
      args: {
        account_id: bettingContractName,
      },
      gas: gas,
      attachedDeposit: "10000000000000000000000",
    });
  } catch (err) {
    console.log("storage deposit error: ", err);
  }
};
// storageDeposit();
async function mint_vex() {
  try {
    await ownerAccount.functionCall({
      contractId: vexAccount,
      methodName: "mint",
      args: {
        account_id: ownerAccount.accountId,
        amount: "99999999000000000000",
      },
      gas,
    });
    // await ownerAccount.functionCall({
    //   contractId: vexAccount,
    //   methodName: "change_max_supply",
    //   args: {
    //     max_supply: 100000000000000000000,
    //   },
    //   gas,
    // });
  } catch (err) {
    console.log("mint vex error: ", err);
  }
}
// mint_vex();
async function util() {
  try {
    const poolInfo = await ownerAccount.viewFunction(
      refContractName,
      "get_pool",
      {
        pool_id: 1916,
      }
    );
    console.log("poolInfo: ", poolInfo);
  } catch (err) {
    console.log("util error: ", err);
  }
}

// util();
async function test_stake_vex() {
  try {
    await ownerAccount.functionCall({
      contractId: vexAccount,
      methodName: "ft_transfer_call",
      args: {
        receiver_id: stakingContract.contractId,
        amount: "100000000000",
        msg: "stake",
      },
      gas,
      attachedDeposit: "1",
    });
    await userAccount.functionCall({
      contractId: vexAccount,
      methodName: "ft_transfer_call",
      args: {
        receiver_id: stakingContract.contractId,
        amount: "200000000000",
        msg: "stake",
      },
      gas,
      attachedDeposit: "1",
    });
    const ownerBalance = await ownerAccount.viewFunction(
      stakingContract.contractId,
      "get_stake_info",
      {
        account_id: ownerAccount.accountId,
      }
    );
    const userBalance = await ownerAccount.viewFunction(
      stakingContract.contractId,
      "get_stake_info",
      {
        account_id: userAccount.accountId,
      }
    );
    console.log("Balances: ", ownerBalance, userBalance);
  } catch (err) {
    console.log("vex stake error: ", err);
  }
}

// test_stake_vex();

async function test_cover_usdc() {
  try {
    const vexOut = await ownerAccount.viewFunction(
      stakingContract.contractId,
      "get_vex_out",
      {}
    );
    console.log("vex_out: ", vexOut);

    await ownerAccount.functionCall({
      contractId: stakingContract.contractId,
      methodName: "cover_usdc",
      args: {
        amount: "10000000000000000000000",
      },
      gas,
      // attachedDeposit: "1",
    });

    const newVexOut = await ownerAccount.viewFunction(
      stakingContract.contractId,
      "get_vex_out",
      {}
    );
    console.log("new_vex_out: ", newVexOut);
  } catch (err) {
    console.log("cover usdc error: ", err);
  }
}
// test_cover_usdc();

const test_usdc_vex_exchange = async () => {
  try {
    const vexout = await ownerAccount.viewFunction(
      stakingContract.contractId,
      "get_vex_out",
      {}
    );
    console.log("vexOUt: ", vexout);
    const claimable = await ownerAccount.viewFunction(
      stakingContract.contractId,
      "get_total",
      {}
    );
    console.log("claimable: ", claimable);
    const vexAmount = await ownerAccount.viewFunction(
      vexAccount,
      "ft_balance_of",
      {
        account_id: stakingContract.contractId,
      }
    );
    const usdcAmount = await ownerAccount.viewFunction(
      usdcAccount,
      "ft_balance_of",
      {
        account_id: stakingContract.contractId,
      }
    );
    console.log("amounts before calling: ", vexAmount, usdcAmount);
    await ownerAccount.functionCall({
      contractId: usdcAccount,
      methodName: "ft_transfer_call",
      args: {
        receiver_id: stakingContract.contractId,
        amount: "20000000000000000000000000",
        msg: "stake",
      },
      gas,
      attachedDeposit: "1",
    });
    const vexAmountAfter = await ownerAccount.viewFunction(
      vexAccount,
      "ft_balance_of",
      {
        account_id: stakingContract.contractId,
      }
    );
    const usdcAmountAfter = await ownerAccount.viewFunction(
      usdcAccount,
      "ft_balance_of",
      {
        account_id: stakingContract.contractId,
      }
    );
    console.log("amounts after calling: ", vexAmountAfter, usdcAmountAfter);
    const newVexout = await ownerAccount.viewFunction(
      stakingContract.contractId,
      "get_vex_out",
      {}
    );
    console.log("newVexout: ", newVexout);
    const newClaimable = await ownerAccount.viewFunction(
      stakingContract.contractId,
      "get_total",
      {}
    );
    console.log("newClaimable: ", newClaimable);
  } catch (err) {
    console.log("test_usdc_vex_exchange error: ", err);
  }
};
test_usdc_vex_exchange();

async function status() {
  try {
    const vexBalance = await ownerAccount.viewFunction(
      vexAccount,
      "ft_balance_of",
      {
        account_id: stakingContract.contractId,
      }
    );
    console.log("vexBalance: ", vexBalance);
  } catch (err) {
    console.log("status check error: ", err);
  }
}
// status();
