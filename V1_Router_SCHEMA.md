## Unified Router Schema (v1)

This document defines a chain-agnostic request/response contract to interact with ZoopX routers on Solana, EVM, Sui, and Aptos. Clients and backends can program against a single shape and map it to chain-native transactions.

### Core principles

- Chain IDs: CAIP-2 (e.g., eip155:1, solana:devnet, sui:mainnet, aptos:mainnet)
- Asset IDs: Prefer CAIP-19 when available
- Account IDs: CAIP-10 for EVM; native formats for Solana/Sui/Aptos
- Amounts: Strings representing base units (no decimals)
- Routing: One request shape; thin mappers convert to chain calls

---

## Request object

```ts
export interface UnifiedRouterRequestV1 {
  id: string;                              // client-assigned identifier
  chainId: string;                         // CAIP-2
  intent: "bridge" | "swap" | "transfer";  // high-level action

  router: {
    programId?: string;    // Solana program ID (base58)
    contract?: string;     // EVM contract address (0x...)
    packageId?: string;    // Sui package object ID
    moduleId?: string;     // Aptos module address::module
  };

  authority: string;        // user wallet address (chain format)
  assetIn: AssetRef;
  assetOut?: AssetRef;      // optional, for swaps/bridges
  amountIn: string;         // base units (as string)

  // Generic fee hint (bps) for chain mappers; Solana uses explicit amounts (see solana.fees)
  feeBps?: number;          // 0-10_000 bp; backends enforce chain caps

  deadline?: number;        // unix seconds (optional)
  adapter?: string;         // target adapter identifier (pubkey/addr)
  payload?: string;         // hex/base64 blob passed to router/adapter

  // Per-chain extensions
  solana?: SolanaExt;
  evm?: EvmExt;
  sui?: SuiExt;
  aptos?: AptosExt;

  // Cross-chain replay protection (string to avoid JS u64 issues)
  nonce?: string;
  simulate?: boolean;       // optional flag for dry-run
}
```

### Asset reference

```ts
export interface AssetRef {
  caip19?: string;
  chainId?: string;  // CAIP-2
  address?: string;  // contract/mint/native
  symbol?: string;
  decimals?: number;
}
```

### Solana extension

```ts
export interface SolanaExt {
  accounts: {
    fromTokenAccount: string;          // ATA(owner=authority, mint)
    feeRecipientToken: string;         // ATA(config.fee_recipient, mint)
    cpiTargetTokenAccount: string;     // ATA(adapter_authority, mint)
    mint: string;                      // mint address
    remaining?: string[];              // additional CPI accounts (must include adapter program id)
  };

  // Explicit fee amounts expected by the router (base units)
  fees?: {
    protocolFee?: string;  // capped at 5 bps of amountIn
    relayerFee?: string;   // combined with protocolFee; total must be <= amountIn
  };

  dstChainId?: number;     // u16; carried into BridgeInitiated event
  nonce?: string;          // u64 as string

  computeUnitLimit?: number;
  priorityFeeMicrolamports?: number;
}
```

Notes for Solana mapping (zoopx_router::universal_bridge_transfer):
- Token program: SPL Token or Token-2022 (ATAs derived with runtime token program id)
- Fees: protocolFee is capped at 5 bps of amountIn; relayerFee is not capped but (protocol+relayer) <= amountIn
- Transfers: combined fees go to `feeRecipientToken`; remainder to `cpiTargetTokenAccount`
- Remaining accounts must include:
  - Target adapter program as read-only, non-signer
  - `cpiTargetTokenAccount`
  - No unexpected signers allowed in remaining accounts
- Custody invariant: `cpiTargetTokenAccount` must equal ATA(adapter_authority, mint) under the runtime token program
- Adapter allowlist: if configured on-chain, adapter program id must be allowlisted
- Payload: arbitrary bytes passed to the adapter CPI; `BridgeInitiated` emits `keccak(payload)`

### EVM extension

```ts
export interface EvmExt {
  gasLimit?: string;
  maxFeePerGas?: string;
  maxPriorityFeePerGas?: string;
  dataOverride?: string;   // raw calldata (optional)
}
```

### Sui extension

```ts
export interface SuiExt {
  objects?: string[];    // object IDs used in PTB
  gasBudget?: string;    // mist
}
```

### Aptos extension

```ts
export interface AptosExt {
  typeArguments?: string[];
  arguments?: any[];
  gasUnitPrice?: string;
  maxGasAmount?: string;
}
```

---

## Response object

```ts
export interface UnifiedRouterResponseV1 {
  id: string;                     // matches request id
  chainId: string;                // CAIP-2
  status: "prepared" | "submitted" | "confirmed" | "failed";

  tx?: {
    hash?: string;                 // tx hash if submitted
    rawTx?: string;                // serialized tx (base64/hex/BCS)
    simulation?: any;              // chain-specific sim output
  };

  error?: {
    code: string;
    message: string;
    details?: any;
  };

  event?: {
    name: string;                  // e.g., BridgeInitiated
    blockTime?: number;
    fields?: Record<string, any>;  // payload hash, fees, amounts, adapter id
  };
}
```

---

## JSON Schema (excerpt)

```json
{
  "$id": "https://zoopx.xyz/schemas/unified-router-request.v1.json",
  "type": "object",
  "required": ["id", "chainId", "intent", "router", "authority", "assetIn", "amountIn"],
  "properties": {
    "id": { "type": "string" },
    "chainId": { "type": "string" },
    "intent": { "enum": ["bridge", "swap", "transfer"] },
    "router": { "type": "object" },
    "authority": { "type": "string" },
    "assetIn": { "$ref": "#/$defs/AssetRef" },
    "amountIn": { "type": "string", "pattern": "^[0-9]+$" }
  }
}
```

---

## Mapping to chains

- Solana: `solana.accounts` map to router accounts; `solana.fees` to on-chain `protocol_fee` and `relayer_fee`; `dstChainId`/`nonce` pass-through to event.
- EVM: `contract` + optional `evm.dataOverride` for calldata construction.
- Sui: `packageId` + `sui.objects` for PTB.
- Aptos: `moduleId` + `aptos.typeArguments/arguments` for entry function.

---

## Usage notes

- If providing `feeBps`, backends should translate to explicit amounts per chain. On Solana, the router enforces a 5 bps cap on `protocolFee` only.
- Adapter must be allowlisted on Solana if the on-chain config has a non-empty allowlist.
- Always include the user authority as the signer.
- Remaining accounts on Solana must include required accounts but have no positional ordering requirement; unexpected signers are forbidden.
- Recommend setting `nonce` and `deadline` for replay protection where supported.

---

## Example (Solana bridge request)

```ts
const req: UnifiedRouterRequestV1 = {
  id: "order-123",
  chainId: "solana:devnet",
  intent: "bridge",
  router: { programId: "654eeCFFpL9koVoFrAhRr1xmvMDq9BnjHYZgc3JxAmNf" },
  authority: "<USER_PUBKEY>",
  assetIn: { address: "<MINT>", chainId: "solana:devnet", decimals: 6 },
  amountIn: "1000000", // 1 token @ 6 decimals
  adapter: "<ADAPTER_PROGRAM_ID>",
  payload: "0x...", // hex/base64
  solana: {
    accounts: {
      fromTokenAccount: "<USER_ATA>",
      feeRecipientToken: "<FEE_RECIPIENT_ATA>",
      cpiTargetTokenAccount: "<CUSTODY_ATA>",
      mint: "<MINT>",
      remaining: ["<ADAPTER_PROGRAM_ID>", "<OTHER_REQUIRED_ACCOUNT_KEYS>..."]
    },
    fees: {
      protocolFee: "250",   // capped at 5 bps of amountIn
      relayerFee: "0"
    },
    dstChainId: 137,         // example target chain id
    nonce: "42"
  }
};
```

---

## Commit message example

```
docs(schema): update Unified Router Schema v1 (Solana fees, dstChainId/nonce, remaining-accounts rules)
```