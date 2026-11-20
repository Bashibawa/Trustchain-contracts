export interface InitParams {
  admin: string;
  escrowContract: string;
}

export interface SetDriverPreferenceParams {
  driver: string;
  toToken: string;
}

export interface ExecuteSettlementSwapParams {
  /** Must be the configured escrow contract; it must authorize the call. */
  caller: string;
  /** Escrow delivery ID this swap pays out, used to correlate swap and escrow record. */
  deliveryId: bigint | number;
  fromToken: string;
  toToken: string;
  recipient: string;
  amount: bigint;
  minAmountOut: bigint;
  /** Unix timestamp; must be within 300 seconds of the current ledger time. */
  deadline: bigint | number;
}
