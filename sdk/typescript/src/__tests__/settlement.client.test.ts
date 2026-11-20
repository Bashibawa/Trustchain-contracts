import { nativeToScVal, scValToNative, xdr } from '@stellar/stellar-sdk';

import { SettlementClient } from '../clients/settlement.client';
import { ContractInvoker } from '../clients/invoker';

/**
 * SettlementClient reaches the chain exclusively through `ContractInvoker.call`,
 * so a test double at that seam exercises the real argument encoding in
 * `clients/invoker.ts` while asserting the exact contract function name each
 * binding targets. Return values are queued per contract function as raw ScVals
 * and decoded the same way `ContractInvoker.call` decodes them.
 */
function mockInvoker(retvals: Record<string, xdr.ScVal[]>): jest.SpyInstance {
  const call = jest.fn(async (functionName: string, args: xdr.ScVal[]) => {
    const queue = retvals[functionName];
    if (!queue || queue.length === 0) {
      return undefined;
    }
    const value = queue.length === 1 ? queue[0] : queue.shift();
    return value === undefined ? undefined : scValToNative(value as xdr.ScVal);
  });
  jest.spyOn(ContractInvoker.prototype, 'call').mockImplementation(call as never);
  return call;
}

// Deterministic placeholder addresses — the invoker is mocked, so only the
// address *encoding* matters, but they must be structurally valid so the
// `Contract`/`Address` constructors in clients/invoker.ts accept them.
const CONTRACT_ID = 'CADQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQP5KR';
const ADMIN = 'GAEQSCIJBEEQSCIJBEEQSCIJBEEQSCIJBEEQSCIJBEEQSCIJBEEQSH7S';
const DRIVER = 'GAEQSCIJBEEQSCIJBEEQSCIJBEEQSCIJBEEQSCIJBEEQSCIJBEEQSH7S';
const TOKEN = 'GAEQSCIJBEEQSCIJBEEQSCIJBEEQSCIJBEEQSCIJBEEQSCIJBEEQSH7S';

function client(): SettlementClient {
  return new SettlementClient(CONTRACT_ID);
}

function decodeAddress(value: xdr.ScVal): string {
  return scValToNative(value) as string;
}

function decodeU64(value: xdr.ScVal): bigint {
  return scValToNative(value) as bigint;
}

afterEach(() => {
  jest.restoreAllMocks();
});

describe('SettlementClient init', () => {
  it('constructs with the given contract id', () => {
    const settlement = client();
    expect(settlement).toBeInstanceOf(SettlementClient);
  });

  it('does not touch the chain on construction', () => {
    const call = jest.spyOn(ContractInvoker.prototype, 'call');
    client();
    expect(call).not.toHaveBeenCalled();
  });
});

describe('SettlementClient getAdmin', () => {
  it('reads get_admin and decodes the admin address', async () => {
    const call = mockInvoker({ get_admin: [nativeToScVal(ADMIN, { type: 'address' })] });

    const admin = await client().getAdmin();

    expect(admin).toBe(ADMIN);
    expect(call).toHaveBeenCalledTimes(1);
    const [fn, args] = call.mock.calls[0];
    expect(fn).toBe('get_admin');
    expect(args).toHaveLength(0);
  });
});

describe('SettlementClient setDriverPreference', () => {
  it('targets set_driver_preference with admin, driver and preference args', async () => {
    const call = mockInvoker({ set_driver_preference: [nativeToScVal(null)] });

    await client().setDriverPreference({
      admin: ADMIN,
      driver: DRIVER,
      preference: 1,
    });

    const [fn, args] = call.mock.calls[0];
    expect(fn).toBe('set_driver_preference');
    expect(args).toHaveLength(3);
    expect(decodeAddress(args[0] as xdr.ScVal)).toBe(ADMIN);
    expect(decodeAddress(args[1] as xdr.ScVal)).toBe(DRIVER);
    expect(scValToNative(args[2] as xdr.ScVal)).toBe(1);
  });

  it('propagates a reverted preference update to the caller', async () => {
    jest
      .spyOn(ContractInvoker.prototype, 'call')
      .mockRejectedValue(new Error('Soroban simulation failed: TrustchainError::Unauthorized'));

    await expect(
      client().setDriverPreference({ admin: ADMIN, driver: DRIVER, preference: 0 }),
    ).rejects.toThrow('Unauthorized');
  });
});

describe('SettlementClient getDriverPreference', () => {
  it('reads get_driver_preference with the driver address', async () => {
    const call = mockInvoker({ get_driver_preference: [nativeToScVal(2)] });

    const preference = await client().getDriverPreference(DRIVER);

    expect(preference).toBe(2);
    const [fn, args] = call.mock.calls[0];
    expect(fn).toBe('get_driver_preference');
    expect(args).toHaveLength(1);
    expect(decodeAddress(args[0] as xdr.ScVal)).toBe(DRIVER);
  });
});

describe('SettlementClient executeSettlementSwap', () => {
  const swapParams = {
    caller: CONTRACT_ID,
    deliveryId: 7101n,
    fromToken: TOKEN,
    toToken: TOKEN,
    recipient: DRIVER,
    amount: 1_000n,
    minAmountOut: 950n,
    deadline: 300n,
  };

  it('targets execute_settlement_swap with the encoded swap args including delivery_id', async () => {
    const call = mockInvoker({ execute_settlement_swap: [nativeToScVal(1_900n, { type: 'i128' })] });

    const amountOut = await client().executeSettlementSwap(swapParams);

    expect(amountOut).toBe(1_900n);

    const [fn, args] = call.mock.calls[0];
    expect(fn).toBe('execute_settlement_swap');
    expect(args).toHaveLength(8);
    expect(decodeAddress(args[0] as xdr.ScVal)).toBe(CONTRACT_ID);
    expect(decodeU64(args[1] as xdr.ScVal)).toBe(7101n);
    expect(decodeAddress(args[2] as xdr.ScVal)).toBe(TOKEN);
    expect(decodeAddress(args[3] as xdr.ScVal)).toBe(TOKEN);
    expect(decodeAddress(args[4] as xdr.ScVal)).toBe(DRIVER);
    expect(scValToNative(args[5] as xdr.ScVal)).toBe(1_000n);
    expect(scValToNative(args[6] as xdr.ScVal)).toBe(950n);
    expect(decodeU64(args[7] as xdr.ScVal)).toBe(300n);
  });

  it('propagates a reverted swap to the caller', async () => {
    jest
      .spyOn(ContractInvoker.prototype, 'call')
      .mockRejectedValue(new Error('Soroban simulation failed: TrustchainError::Unauthorized'));

    await expect(client().executeSettlementSwap(swapParams)).rejects.toThrow('Unauthorized');
  });
});
