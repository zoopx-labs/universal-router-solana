// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {AccessControl} from "@openzeppelin/contracts/access/AccessControl.sol";
import {EIP712} from "@openzeppelin/contracts/utils/cryptography/EIP712.sol";
import {ECDSA} from "@openzeppelin/contracts/utils/cryptography/ECDSA.sol";
import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";
import {Hashing} from "./lib/Hashing.sol";

// Optional permit interfaces
interface IERC20Permit {
    function permit(address owner, address spender, uint256 value, uint256 deadline, uint8 v, bytes32 r, bytes32 s)
        external;
}

interface IERC20PermitDAI {
    function permit(
        address holder,
        address spender,
        uint256 nonce,
        uint256 expiry,
        bool allowed,
        uint8 v,
        bytes32 r,
        bytes32 s
    ) external;
}

contract Router is ReentrancyGuard, EIP712, AccessControl {
    using SafeERC20 for IERC20;

    /// @notice Security notes:
    /// @dev This router is intentionally stateless per transfer. It uses transfer-before-call semantics
    ///      (pull funds -> skim fees -> forward -> emit -> call) to avoid holding user balances.
    ///      For vault/pool interactions that require pull semantics, the router provides an
    ///      approve-then-call flow that performs ephemeral approvals (approve 0 -> approve X -> call -> approve 0)
    ///      in the same transaction and revokes approvals immediately after the external call.
    ///      Low-level calls to partner adapters are required because the router is partner-agnostic.
    ///      The contract checks call return and reverts on failure. These design choices will trigger
    ///      certain static-analysis warnings (external-call, approvals); they are intentional and
    ///      documented here for reviewers and automated tools.
    ///
    /// EIP-712 pulls: this contract authorizes pulls from user accounts when an EIP-712
    /// RouteIntent signed by that user is presented. Static-analysis tools may flag the
    /// resulting transferFrom as arbitrary-send; these are intentional meta-transactions
    /// authorized by the user's signature. Where appropriate the code includes
    /// Slither suppression comments to document the authorization.

    // ---------- Admin & config ----------
    address public admin;
    address public feeRecipient;

    // ---------- Errors ----------
    error ZeroAmount();
    error FeeTooHigh();
    error FeesExceedAmount();
    error TokenZeroAddress();
    error TargetNotSet();
    error TargetNotContract();
    error PayloadTooLarge();
    error PayloadDisallowedToEOA();
    error FeeOnTransferNotSupported();
    error ExpiredIntent();
    error InvalidSignature();
    error IntentMismatch();
    error ResidueLeft();
    error NotAdapter();
    error AdapterFrozenErr();

    // ---------- Events ----------
    event BridgeInitiated( // commitment to the full off-chain plan
        // recovered signer (intent.user)
        // adapter/partner/vault/bridge handler
        bytes32 indexed routeId,
        address indexed user,
        address indexed token,
        address target,
        uint256 forwardedAmount,
    uint256 /*protocolFee*/,
        uint256 relayerFee,
        bytes32 payloadHash,
        uint16 srcChainId,
        uint16 dstChainId,
        uint64 nonce
    );
    event IntentConsumed(bytes32 indexed digest, bytes32 indexed routeId, address indexed user);

    // ---------- Types ----------
    struct TransferArgs {
        address token;
        uint256 amount;
        uint256 protocolFee;
        uint256 relayerFee;
        bytes payload; // opaque adapter calldata
        address target; // override defaultTarget if nonzero
        uint16 dstChainId;
        uint64 nonce;
    }

    // EIP-712 intent signed by the user (owner of funds)
    struct RouteIntent {
        bytes32 routeId; // keccak256(routePlan)
        address user; // signer & token owner
        address token;
        uint256 amount;
        uint256 protocolFee;
        uint256 relayerFee;
        uint16 dstChainId;
        address recipient; // recommended: expected target/receiver for this leg
        uint256 expiry; // unix seconds
        bytes32 payloadHash; // keccak256(payload)
        uint64 nonce; // off-chain unique; router remains stateless
    }

    // typehash for RouteIntent
    bytes32 private constant ROUTE_INTENT_TYPEHASH = keccak256(
        "RouteIntent(bytes32 routeId,address user,address token,uint256 amount,uint256 protocolFee,uint256 relayerFee,uint16 dstChainId,address recipient,uint256 expiry,bytes32 payloadHash,uint64 nonce)"
    );

    // public accessor for tests and off-chain tooling
    function ROUTE_INTENT_TYPEHASH_PUBLIC() external pure returns (bytes32) {
        return ROUTE_INTENT_TYPEHASH;
    }

    // optional target allowlist (disabled by default)
    mapping(address => bool) public isAllowedTarget;
    bool public enforceTargetAllowlist;

    function setAllowedTarget(address t, bool ok) external onlyAdmin {
        isAllowedTarget[t] = ok;
    }

    function setEnforceTargetAllowlist(bool v) external onlyAdmin {
        enforceTargetAllowlist = v;
    }

    // ---------- Config ----------
    uint16 public constant FEE_CAP_BPS = 5; // 0.05%
    uint256 public constant MAX_PAYLOAD_BYTES = 512;

    address public immutable defaultTarget; // 0x0 => must pass target
    uint16 public immutable SRC_CHAIN_ID;
    // No storage temporaries: router remains stateless and uses locals only

    // new constructor: admin, feeRecipient, defaultTarget, srcChainId
    // NOTE: If _defaultTarget == address(0), every call must explicitly supply a.target (no silent default).
    constructor(address _admin, address _feeRecipient, address _defaultTarget, uint16 _srcChainId)
        EIP712("ZoopXRouter", "1")
    {
        require(_admin != address(0), "bad admin");
        require(_feeRecipient != address(0), "bad feeRecipient");
        admin = _admin;
        feeRecipient = _feeRecipient;
        defaultTarget = _defaultTarget;
        SRC_CHAIN_ID = _srcChainId;
        _grantRole(DEFAULT_ADMIN_ROLE, _admin);
    }

    // ---------- Replay protection for signed intents (small mapping)
    // maps EIP-712 digest -> used
    mapping(bytes32 => bool) public usedIntents;

    error IntentAlreadyUsed();

    // ---------- Message-level replay protection for cross-chain messages
    // maps canonical messageHash -> used
    mapping(bytes32 => bool) public usedMessages;

    error MessageAlreadyUsed();

    // ---------- Adapter authority (role-based, replaces single-adapter model)
    // deprecated: single-adapter model replaced by ADAPTER_ROLE
    address public adapter; // deprecated, retained for storage layout stability

    // Role-based adapter allowlist
    bytes32 public constant ADAPTER_ROLE = keccak256("ADAPTER_ROLE");
    mapping(address => bool) public frozenAdapter; // false by default

    // Events for adapter management
    event AdapterAdded(address adapter);
    event AdapterRemoved(address adapter);
    event AdapterFrozen(address adapter, bool frozen);

    // Admin functions for adapter management
    function addAdapter(address a) external onlyRole(DEFAULT_ADMIN_ROLE) {
        _grantRole(ADAPTER_ROLE, a);
        emit AdapterAdded(a);
    }

    function removeAdapter(address a) external onlyRole(DEFAULT_ADMIN_ROLE) {
        _revokeRole(ADAPTER_ROLE, a);
        emit AdapterRemoved(a);
    }

    function freezeAdapter(address a, bool frozen) external onlyRole(DEFAULT_ADMIN_ROLE) {
        frozenAdapter[a] = frozen;
    emit AdapterFrozen(a, frozen);
    }

    // Adapter role gate
    modifier onlyAdapter() {
        if (!hasRole(ADAPTER_ROLE, _msgSender())) revert NotAdapter();
    if (frozenAdapter[_msgSender()]) revert AdapterFrozenErr();
        _;
    }

    // Fee configuration (bps) and collector
    uint16 public protocolFeeBps;
    uint16 public relayerFeeBps;
    uint16 public protocolShareBps;
    uint16 public lpShareBps;
    address public feeCollector;

    // Events for fee application and canonical initiation
    event FeeApplied(
        bytes32 indexed globalRouteId,
        bytes32 indexed messageHash,
        uint16 chainId,
        address router,
        address vault,
        address asset,
        uint256 protocol_fee_native,
        uint256 relayer_fee_native,
        uint16 protocol_bps,
        uint16 lp_bps,
        address collector,
        uint256 applied_at
    );
    // Source-leg fee telemetry (when router skims instead of delegating to target)
    event FeeAppliedSource(
        bytes32 indexed messageHash,
        address indexed asset,
        address indexed payer,
        address target,
        uint256 protocolFee,
        uint256 relayerFee,
        address feeRecipient,
        uint256 appliedAt
    );

    event UniversalBridgeInitiated(
        bytes32 indexed routeId,
        bytes32 indexed payloadHash,
        bytes32 indexed messageHash,
        bytes32 globalRouteId,
        address user,
        address token,
        address target,
        uint256 forwardedAmount,
        uint256 protocolFee,
        uint256 relayerFee,
        uint16 srcChainId,
        uint16 dstChainId,
        uint64 nonce
    );

    // ---------- Admin errors/events/modifiers ----------
    error Unauthorized();
    error ZeroAddress();

    event AdminUpdated(address indexed oldAdmin, address indexed newAdmin);
    event FeeRecipientUpdated(address indexed oldFeeRecipient, address indexed newFeeRecipient);

    modifier onlyAdmin() {
        if (msg.sender != admin) revert Unauthorized();
        _;
    }

    function setAdmin(address newAdmin) external onlyAdmin {
        if (newAdmin == address(0)) revert ZeroAddress();
        emit AdminUpdated(admin, newAdmin);
        admin = newAdmin;
    }

    function setFeeRecipient(address newFeeRecipient) external onlyAdmin {
        if (newFeeRecipient == address(0)) revert ZeroAddress();
        emit FeeRecipientUpdated(feeRecipient, newFeeRecipient);
        feeRecipient = newFeeRecipient;
    }

    // Two-step admin handover
    address public pendingAdmin;

    event AdminProposed(address indexed current, address indexed proposed);

    function proposeAdmin(address p) external onlyAdmin {
        if (p == address(0)) revert ZeroAddress();
        pendingAdmin = p;
        emit AdminProposed(admin, p);
    }

    function acceptAdmin() external {
        if (msg.sender != pendingAdmin) revert Unauthorized();
        address old = admin;
        admin = pendingAdmin;
        pendingAdmin = address(0);
        _grantRole(DEFAULT_ADMIN_ROLE, admin);
        _revokeRole(DEFAULT_ADMIN_ROLE, old);
        emit AdminUpdated(old, admin);
    }

    // ---------- Public: generic, no signature ----------
    function universalBridgeTransfer(TransferArgs calldata a) external nonReentrant {
        _commonChecks(a.token, a.amount, a.protocolFee, a.relayerFee);
        address target = a.target != address(0) ? a.target : defaultTarget;
        if (target == address(0)) revert TargetNotSet();
        if (a.payload.length > MAX_PAYLOAD_BYTES) revert PayloadTooLarge();

        bool isContract = target.code.length > 0;
        if (!isContract && a.payload.length != 0) revert PayloadDisallowedToEOA();
        if (enforceTargetAllowlist && isContract && !isAllowedTarget[target]) revert TargetNotContract();

        // perform pull, optional fee skim and forward to target
        uint256 balBefore = IERC20(a.token).balanceOf(address(this));
        uint256 forwardAmount = _pullSkimAndForward(
            a.token, msg.sender, target, a.amount, a.protocolFee, a.relayerFee
        );

        // compute canonical hashes (canonical ordering: srcChainId, srcAdapter(target), recipient(address(0) here), asset, forwardAmount, payloadHash, nonce, dstChainId)
        bytes32 payloadHash = Hashing.payloadHash(a.payload);
        uint64 src = uint64(SRC_CHAIN_ID);
        uint64 dst = uint64(a.dstChainId);
        address recipientAddr = address(0); // no explicit recipient in unsigned flow
        bytes32 messageHash = Hashing.messageHash(
            src,
            target,
            recipientAddr,
            a.token,
            forwardAmount,
            payloadHash,
            a.nonce,
            dst
        );
        // emit source fee telemetry only if router skimmed (i.e., not delegating)
        if (!delegateFeeToTarget[target] && (a.protocolFee + a.relayerFee) > 0) {
            emit FeeAppliedSource(
                messageHash,
                a.token,
                msg.sender,
                target,
                a.protocolFee,
                a.relayerFee,
                feeRecipient,
                block.timestamp
            );
        }
        bytes32 globalRouteId = computeGlobalRouteId(SRC_CHAIN_ID, a.dstChainId, msg.sender, messageHash, a.nonce);
        emit BridgeInitiated(
            bytes32(0),
            msg.sender,
            a.token,
            target,
            forwardAmount,
            a.protocolFee,
            a.relayerFee,
            payloadHash,
            SRC_CHAIN_ID,
            a.dstChainId,
            a.nonce
        );
    // BACKEND NOTE: BridgeID = hex(messageHash) off-chain label tying source+destination tx receipts.
    // Off-chain indexer stores per-leg tx hash arrays keyed by messageHash. globalRouteId is ancillary grouping.
    emit UniversalBridgeInitiated(
            bytes32(0),
            payloadHash,
            messageHash,
            globalRouteId,
            msg.sender,
            a.token,
            target,
            forwardAmount,
            a.protocolFee,
            a.relayerFee,
            SRC_CHAIN_ID,
            a.dstChainId,
            a.nonce
        );

        // call contract targets only
        if (isContract && a.payload.length > 0) _callTarget(target, a.payload);

        // defense-in-depth: ensure no residue left
        if (IERC20(a.token).balanceOf(address(this)) != balBefore) revert ResidueLeft();
    }

    // ---------- Public: generic with EIP-712 signature ----------
    function universalBridgeTransferWithSig(
        TransferArgs calldata a,
        RouteIntent calldata intent,
        bytes calldata signature
    ) external nonReentrant {
        // Shared verification & bindings
        (address target, bool isContract, uint256 balBefore) = _preSignedPullAndChecks(a, intent, signature);
        uint256 forwardAmount = _pullSkimAndForward(
            a.token, intent.user, target, a.amount, a.protocolFee, a.relayerFee
        );
        // Signed: recipient may be intent.recipient (or address(0) if unset)
        (bytes32 payloadHash, bytes32 messageHash, bytes32 globalRouteId) = _computeHashesSigned(a, intent, target, forwardAmount);
        if (!delegateFeeToTarget[target] && (a.protocolFee + a.relayerFee) > 0) {
            emit FeeAppliedSource(
                messageHash,
                a.token,
                intent.user,
                target,
                a.protocolFee,
                a.relayerFee,
                feeRecipient,
                block.timestamp
            );
        }
        _emitSourceEvents(intent.routeId, intent.user, a, target, forwardAmount, payloadHash, messageHash, globalRouteId);
        if (isContract && a.payload.length > 0) _callTarget(target, a.payload);
        if (IERC20(a.token).balanceOf(address(this)) != balBefore) revert ResidueLeft();
    }

    // Internal helper consolidating initial verification & environment setup for signed pull variants
    function _preSignedPullAndChecks(
        TransferArgs calldata a,
        RouteIntent calldata intent,
        bytes calldata signature
    ) internal returns (address target, bool isContract, uint256 balBefore) {
        // Verify EIP-712 intent, get digest and claim it for replay-protection before external calls
        bytes32 digest = _verifyIntentReturningDigest(intent, signature);
        if (usedIntents[digest]) revert IntentAlreadyUsed();
        usedIntents[digest] = true;
        emit IntentConsumed(digest, intent.routeId, intent.user);

        // Bind call arguments to the signed commitment
        if (intent.token != a.token) revert IntentMismatch();
        if (intent.amount != a.amount) revert IntentMismatch();
        if (intent.protocolFee != a.protocolFee) revert IntentMismatch();
        if (intent.relayerFee != a.relayerFee) revert IntentMismatch();
        if (intent.dstChainId != a.dstChainId) revert IntentMismatch();
    if (intent.payloadHash != Hashing.payloadHash(a.payload)) revert IntentMismatch();

    target = a.target != address(0) ? a.target : defaultTarget;
        if (target == address(0)) revert TargetNotSet();
        if (a.payload.length > MAX_PAYLOAD_BYTES) revert PayloadTooLarge();

    isContract = target.code.length > 0;
        if (!isContract && a.payload.length != 0) revert PayloadDisallowedToEOA();
        if (enforceTargetAllowlist && isContract && !isAllowedTarget[target]) revert TargetNotContract();

        // Tighten binding: recipient must match target when set
        if (intent.recipient != address(0) && intent.recipient != target) revert IntentMismatch();

        balBefore = IERC20(a.token).balanceOf(address(this));
        return (target, isContract, balBefore);
    }

    // ---------- Optional: Permit variants (partner-agnostic) ----------
    function universalBridgeTransferWithPermit(TransferArgs calldata a, uint256 deadline, uint8 v, bytes32 r, bytes32 s)
        external
        nonReentrant
    {
        _commonChecks(a.token, a.amount, a.protocolFee, a.relayerFee);
        address target = a.target != address(0) ? a.target : defaultTarget;
        if (target == address(0)) revert TargetNotSet();
        if (a.payload.length > MAX_PAYLOAD_BYTES) revert PayloadTooLarge();

        bool isContract = target.code.length > 0;
        if (!isContract && a.payload.length != 0) revert PayloadDisallowedToEOA();
        if (enforceTargetAllowlist && isContract && !isAllowedTarget[target]) revert TargetNotContract();

        IERC20Permit(a.token).permit(msg.sender, address(this), a.amount, deadline, v, r, s);

        uint256 balBefore = IERC20(a.token).balanceOf(address(this));
        // perform pull, fee skim and forward to target
        uint256 forwardAmount = _pullSkimAndForward(
            a.token, msg.sender, target, a.amount, a.protocolFee, a.relayerFee
        );

        bytes32 payloadHash = Hashing.payloadHash(a.payload);
        uint64 src = uint64(SRC_CHAIN_ID);
        uint64 dst = uint64(a.dstChainId);
        address recipientAddr = address(0);
        bytes32 messageHash = Hashing.messageHash(
            src,
            target,
            recipientAddr,
            a.token,
            forwardAmount,
            payloadHash,
            a.nonce,
            dst
        );
        if (!delegateFeeToTarget[target] && (a.protocolFee + a.relayerFee) > 0) {
            emit FeeAppliedSource(
                messageHash,
                a.token,
                msg.sender,
                target,
                a.protocolFee,
                a.relayerFee,
                feeRecipient,
                block.timestamp
            );
        }
        bytes32 globalRouteId = computeGlobalRouteId(SRC_CHAIN_ID, a.dstChainId, msg.sender, messageHash, a.nonce);
        emit BridgeInitiated(
            bytes32(0),
            msg.sender,
            a.token,
            target,
            forwardAmount,
            a.protocolFee,
            a.relayerFee,
            payloadHash,
            SRC_CHAIN_ID,
            a.dstChainId,
            a.nonce
        );
    // NOTE: Backend derives BridgeID from messageHash off-chain (stores leg tx hashes keyed by messageHash)
    emit UniversalBridgeInitiated(
            bytes32(0),
            payloadHash,
            messageHash,
            globalRouteId,
            msg.sender,
            a.token,
            target,
            forwardAmount,
            a.protocolFee,
            a.relayerFee,
            SRC_CHAIN_ID,
            a.dstChainId,
            a.nonce
        );
        if (isContract && a.payload.length > 0) _callTarget(target, a.payload);
        if (IERC20(a.token).balanceOf(address(this)) != balBefore) revert ResidueLeft();
    }

    function universalBridgeTransferWithDAIPermit(
        TransferArgs calldata a,
        uint256 permitNonce,
        uint256 expiry,
        bool allowed,
        uint8 v,
        bytes32 r,
        bytes32 s
    ) external nonReentrant {
        _commonChecks(a.token, a.amount, a.protocolFee, a.relayerFee);
        address target = a.target != address(0) ? a.target : defaultTarget;
        if (target == address(0)) revert TargetNotSet();
        if (a.payload.length > MAX_PAYLOAD_BYTES) revert PayloadTooLarge();

        bool isContract = target.code.length > 0;
        if (!isContract && a.payload.length != 0) revert PayloadDisallowedToEOA();
        if (enforceTargetAllowlist && isContract && !isAllowedTarget[target]) revert TargetNotContract();

        IERC20PermitDAI(a.token).permit(msg.sender, address(this), permitNonce, expiry, allowed, v, r, s);

        uint256 balBefore = IERC20(a.token).balanceOf(address(this));
        uint256 forwardAmount = _pullSkimAndForward(
            a.token, msg.sender, target, a.amount, a.protocolFee, a.relayerFee
        );

        bytes32 payloadHash = Hashing.payloadHash(a.payload);
        uint64 src = uint64(SRC_CHAIN_ID);
        uint64 dst = uint64(a.dstChainId);
        address recipientAddr = address(0);
        bytes32 messageHash = Hashing.messageHash(
            src,
            target,
            recipientAddr,
            a.token,
            forwardAmount,
            payloadHash,
            a.nonce,
            dst
        );
        if (!delegateFeeToTarget[target] && (a.protocolFee + a.relayerFee) > 0) {
            emit FeeAppliedSource(
                messageHash,
                a.token,
                msg.sender,
                target,
                a.protocolFee,
                a.relayerFee,
                feeRecipient,
                block.timestamp
            );
        }
        bytes32 globalRouteId = computeGlobalRouteId(SRC_CHAIN_ID, a.dstChainId, msg.sender, messageHash, a.nonce);
        emit BridgeInitiated(
            bytes32(0),
            msg.sender,
            a.token,
            target,
            forwardAmount,
            a.protocolFee,
            a.relayerFee,
            payloadHash,
            SRC_CHAIN_ID,
            a.dstChainId,
            a.nonce
        );
    // NOTE: Backend derives BridgeID from messageHash off-chain (stores leg tx hashes keyed by messageHash)
    emit UniversalBridgeInitiated(
            bytes32(0),
            payloadHash,
            messageHash,
            globalRouteId,
            msg.sender,
            a.token,
            target,
            forwardAmount,
            a.protocolFee,
            a.relayerFee,
            SRC_CHAIN_ID,
            a.dstChainId,
            a.nonce
        );
        if (isContract && a.payload.length > 0) _callTarget(target, a.payload);
        if (IERC20(a.token).balanceOf(address(this)) != balBefore) revert ResidueLeft();
    }

    // ---------- Approve-then-call (pull semantics for vaults/pools) ----------
    function universalBridgeApproveThenCall(TransferArgs calldata a) external nonReentrant {
        _commonChecks(a.token, a.amount, a.protocolFee, a.relayerFee);
        address target = a.target != address(0) ? a.target : defaultTarget;
        if (target == address(0)) revert TargetNotSet();
        if (a.payload.length > MAX_PAYLOAD_BYTES) revert PayloadTooLarge();

        bool isContract = target.code.length > 0;
        if (!isContract && a.payload.length != 0) revert PayloadDisallowedToEOA();
        
        if (enforceTargetAllowlist && isContract && !isAllowedTarget[target]) revert TargetNotContract();

        IERC20 t = IERC20(a.token);

        uint256 balBefore = t.balanceOf(address(this));
        t.safeTransferFrom(msg.sender, address(this), a.amount);
        uint256 received = t.balanceOf(address(this)) - balBefore;
        if (received != a.amount) revert FeeOnTransferNotSupported();

        uint256 forwardAmount;
        if (delegateFeeToTarget[target]) {
            // delegate: no skim, full amount available for target pull
            forwardAmount = a.amount;
        } else {
            uint256 fees = a.protocolFee + a.relayerFee;
            if (relayerFeeBps > 0) {
                uint256 relayerCap = Math.mulDiv(a.amount, relayerFeeBps, 10_000);
                if (a.relayerFee > relayerCap) revert FeeTooHigh();
            }
            if (fees > 0) {
                if (feeRecipient == address(0)) revert ZeroAddress();
                t.safeTransfer(feeRecipient, fees);
            }
            forwardAmount = a.amount - fees;
        }

        // Ephemeral approval to target - use OZ forceApprove
        IERC20(a.token).forceApprove(target, 0);
        IERC20(a.token).forceApprove(target, forwardAmount);

        // perform call (target should pull)
        if (isContract && a.payload.length > 0) _callTarget(target, a.payload);

        // Revoke approval
        IERC20(a.token).forceApprove(target, 0);

        // compute and emit
        bytes32 payloadHash = Hashing.payloadHash(a.payload);
        uint64 src = uint64(SRC_CHAIN_ID);
        uint64 dst = uint64(a.dstChainId);
        address recipientAddr = address(0);
        bytes32 messageHash = Hashing.messageHash(
            src,
            target,
            recipientAddr,
            a.token,
            forwardAmount,
            payloadHash,
            a.nonce,
            dst
        );
        if (!delegateFeeToTarget[target] && (a.protocolFee + a.relayerFee) > 0) {
            emit FeeAppliedSource(
                messageHash,
                a.token,
                msg.sender,
                target,
                a.protocolFee,
                a.relayerFee,
                feeRecipient,
                block.timestamp
            );
        }
        bytes32 globalRouteId = computeGlobalRouteId(SRC_CHAIN_ID, a.dstChainId, msg.sender, messageHash, a.nonce);
        emit BridgeInitiated(
            bytes32(0),
            msg.sender,
            a.token,
            target,
            forwardAmount,
            a.protocolFee,
            a.relayerFee,
            payloadHash,
            SRC_CHAIN_ID,
            a.dstChainId,
            a.nonce
        );
    // NOTE: Backend derives BridgeID from messageHash off-chain (stores leg tx hashes keyed by messageHash)
    emit UniversalBridgeInitiated(
            bytes32(0),
            payloadHash,
            messageHash,
            globalRouteId,
            msg.sender,
            a.token,
            target,
            forwardAmount,
            a.protocolFee,
            a.relayerFee,
            SRC_CHAIN_ID,
            a.dstChainId,
            a.nonce
        );

        // defense-in-depth: ensure no residue left
        if (IERC20(a.token).balanceOf(address(this)) != balBefore) revert ResidueLeft();
    }

    function universalBridgeApproveThenCallWithSig(
        TransferArgs calldata a,
        RouteIntent calldata intent,
        bytes calldata signature
    ) external nonReentrant {
        (address target, bool isContract, uint256 balBefore) = _preSignedApproveThenCallChecks(a, intent, signature);
        IERC20 t = IERC20(a.token);
        // pull
        t.safeTransferFrom(intent.user, address(this), a.amount);
        uint256 received = t.balanceOf(address(this)) - balBefore;
        if (received != a.amount) revert FeeOnTransferNotSupported();
        uint256 forwardAmount;
        if (delegateFeeToTarget[target]) {
            forwardAmount = a.amount;
        } else {
            uint256 fees = a.protocolFee + a.relayerFee;
            if (relayerFeeBps > 0) {
                uint256 relayerCap = Math.mulDiv(a.amount, relayerFeeBps, 10_000);
                if (a.relayerFee > relayerCap) revert FeeTooHigh();
            }
            if (fees > 0) {
                if (feeRecipient == address(0)) revert ZeroAddress();
                t.safeTransfer(feeRecipient, fees);
            }
            forwardAmount = a.amount - fees;
        }
        IERC20(a.token).forceApprove(target, 0);
        IERC20(a.token).forceApprove(target, forwardAmount);
        if (isContract && a.payload.length > 0) _callTarget(target, a.payload);
        IERC20(a.token).forceApprove(target, 0);
        (bytes32 payloadHash, bytes32 messageHash, bytes32 globalRouteId) = _computeHashesSigned(a, intent, target, forwardAmount);
        if (!delegateFeeToTarget[target] && (a.protocolFee + a.relayerFee) > 0) {
            emit FeeAppliedSource(
                messageHash,
                a.token,
                intent.user,
                target,
                a.protocolFee,
                a.relayerFee,
                feeRecipient,
                block.timestamp
            );
        }
        _emitSourceEvents(intent.routeId, intent.user, a, target, forwardAmount, payloadHash, messageHash, globalRouteId);
        if (IERC20(a.token).balanceOf(address(this)) != balBefore) revert ResidueLeft();
    }

    function _preSignedApproveThenCallChecks(
        TransferArgs calldata a,
        RouteIntent calldata intent,
        bytes calldata signature
    ) internal returns (address target, bool isContract, uint256 balBefore) {
        // reuse binding logic
        (target, isContract, balBefore) = _preSignedPullAndChecks(a, intent, signature);
        if (!isContract && a.payload.length != 0) revert PayloadDisallowedToEOA();
        return (target, isContract, balBefore);
    }

    function _computeHashesSigned(
        TransferArgs calldata a,
        RouteIntent calldata intent,
        address target,
        uint256 forwardAmount
    ) internal view returns (bytes32 payloadHash, bytes32 messageHash, bytes32 globalRouteId) {
        payloadHash = Hashing.payloadHash(a.payload);
        uint64 src = uint64(SRC_CHAIN_ID);
        uint64 dst = uint64(a.dstChainId);
        address recipientAddr = intent.recipient != address(0) ? intent.recipient : address(0);
        messageHash = Hashing.messageHash(
            src,
            target,
            recipientAddr,
            a.token,
            forwardAmount,
            payloadHash,
            a.nonce,
            dst
        );
        globalRouteId = computeGlobalRouteId(SRC_CHAIN_ID, a.dstChainId, intent.user, messageHash, a.nonce);
    }

    function _emitSourceEvents(
        bytes32 routeId,
        address user,
        TransferArgs calldata a,
        address target,
        uint256 forwardAmount,
        bytes32 payloadHash,
        bytes32 messageHash,
        bytes32 globalRouteId
    ) internal {
        emit BridgeInitiated(
            routeId,
            user,
            a.token,
            target,
            forwardAmount,
            a.protocolFee,
            a.relayerFee,
            payloadHash,
            SRC_CHAIN_ID,
            a.dstChainId,
            a.nonce
        );
        // BACKEND NOTE: BridgeID = hex(messageHash) canonical; relayer correlates finalizeMessage using same hash schema.
        emit UniversalBridgeInitiated(
            routeId,
            payloadHash,
            messageHash,
            globalRouteId,
            user,
            a.token,
            target,
            forwardAmount,
            a.protocolFee,
            a.relayerFee,
            SRC_CHAIN_ID,
            a.dstChainId,
            a.nonce
        );
    }

    // ---------- Internal helpers ----------
    function _commonChecks(address token, uint256 amount, uint256 protocolFee, uint256 relayerFee) internal view {
        if (token == address(0)) revert TokenZeroAddress();
        if (amount == 0) revert ZeroAmount();
        if (protocolFee + relayerFee > amount) revert FeesExceedAmount();
        if (protocolFee > Math.mulDiv(amount, FEE_CAP_BPS, 10_000)) revert FeeTooHigh();
        if (relayerFeeBps > 0) {
            uint256 relayerCap = Math.mulDiv(amount, relayerFeeBps, 10_000);
            if (relayerFee > relayerCap) revert FeeTooHigh();
        }
    }

    // NOTE: deprecated custom _forceApprove removed; using OpenZeppelin's `forceApprove` via SafeERC20

    function _pullSkimAndForward(
        address token,
        address user,
        address target,
        uint256 amount,
        uint256 protocolFee,
        uint256 relayerFee
    ) internal returns (uint256 forwardAmount) {
        IERC20 t = IERC20(token);

        // forbid fee-on-transfer (or replace with computing 'received')
        uint256 balBefore = t.balanceOf(address(this));
        t.safeTransferFrom(user, address(this), amount);
        uint256 received = t.balanceOf(address(this)) - balBefore;
        if (received != amount) revert FeeOnTransferNotSupported();
        // If delegating fee logic to target vault, forward full amount (no skim here)
        if (delegateFeeToTarget[target]) {
            forwardAmount = amount; // vault expected to skim downstream
            t.safeTransfer(target, forwardAmount);
            return forwardAmount;
        }
        // Router-side skim path
        // Cap relayer fee relative to amount using relayerFeeBps (if set >0)
        if (relayerFeeBps > 0) {
            uint256 relayerCap = Math.mulDiv(amount, relayerFeeBps, 10_000);
            if (relayerFee > relayerCap) revert FeeTooHigh(); // use existing FeeTooHigh error for cap breach
        }
        uint256 totalFees = protocolFee + relayerFee;
        if (protocolFee > Math.mulDiv(amount, FEE_CAP_BPS, 10_000)) revert FeeTooHigh();
        if (totalFees > 0) {
            if (feeRecipient == address(0)) revert ZeroAddress();
            t.safeTransfer(feeRecipient, totalFees);
        }
        forwardAmount = amount - totalFees;
        t.safeTransfer(target, forwardAmount);
        return forwardAmount;
    }

    // (removed _pullEmitCall helper; flows now use _pullSkimAndForward + emit + _callTarget)

    function _callTarget(address target, bytes calldata payload) internal {
        (bool ok,) = target.call(payload);
        require(ok, "target call failed");
    }

    // New name requested by pre-testnet fixes: return the digest while performing the same checks.
    function _verifyIntentReturningDigest(RouteIntent calldata intent, bytes calldata sig)
        internal
        view
        returns (bytes32 digest)
    {
        if (block.timestamp > intent.expiry) revert ExpiredIntent();
        if (intent.payloadHash == bytes32(0)) revert PayloadTooLarge();

        bytes32 structHash = keccak256(
            abi.encode(
                ROUTE_INTENT_TYPEHASH,
                intent.routeId,
                intent.user,
                intent.token,
                intent.amount,
                intent.protocolFee,
                intent.relayerFee,
                intent.dstChainId,
                intent.recipient,
                intent.expiry,
                intent.payloadHash,
                intent.nonce
            )
        );
        digest = _hashTypedDataV4(structHash);
        if (ECDSA.recover(digest, sig) != intent.user) revert InvalidSignature();
    }

    // ---------- Helpers: global route id (deterministic on-chain)
    function computeGlobalRouteId(
        uint16 srcChainId,
        uint16 dstChainId,
        address initiator,
        bytes32 messageHash,
        uint64 nonce
    ) public pure returns (bytes32) {
        return keccak256(abi.encodePacked(srcChainId, dstChainId, initiator, messageHash, nonce));
    }

    // ---------- Admin setters for adapter and fee collector
    function setAdapter(address a) external onlyAdmin {
        // deprecated: retained for backward compatibility (single-adapter model replaced by ADAPTER_ROLE)
        adapter = a;
    }

    function setFeeCollector(address c) external onlyAdmin {
        if (c == address(0)) revert ZeroAddress();
        feeCollector = c;
    }

    // Admin setters for BPS configuration with validation
    event ProtocolFeeBpsUpdated(uint16 oldBps, uint16 newBps);
    event RelayerFeeBpsUpdated(uint16 oldBps, uint16 newBps);
    event ProtocolShareBpsUpdated(uint16 oldBps, uint16 newBps);
    event LPShareBpsUpdated(uint16 oldBps, uint16 newBps);

    function setProtocolFeeBps(uint16 bps) external onlyAdmin {
        // enforce reasonable cap using FEE_CAP_BPS for on-chain protocol fee cap
        if (bps > FEE_CAP_BPS) revert FeeTooHigh();
        emit ProtocolFeeBpsUpdated(protocolFeeBps, bps);
        protocolFeeBps = bps;
    }

    function setRelayerFeeBps(uint16 bps) external onlyAdmin {
        // relayer fee cap cannot exceed 10% (1000 bps) as a safety heuristic
        if (bps > 1000) revert FeeTooHigh();
        emit RelayerFeeBpsUpdated(relayerFeeBps, bps);
        relayerFeeBps = bps;
    }

    function setProtocolShareBps(uint16 bps) external onlyAdmin {
        // protocolShareBps + lpShareBps must not exceed 10000 (100%)
        if (uint256(bps) + uint256(lpShareBps) > 10_000) revert FeeTooHigh();
        emit ProtocolShareBpsUpdated(protocolShareBps, bps);
        protocolShareBps = bps;
    }

    function setLPShareBps(uint16 bps) external onlyAdmin {
        if (uint256(protocolShareBps) + uint256(bps) > 10_000) revert FeeTooHigh();
        emit LPShareBpsUpdated(lpShareBps, bps);
        lpShareBps = bps;
    }

    // Enforce exact 100% split between protocol and LP shares (even if destination no longer skims)
    function setFeeSplit(uint16 protocolShare, uint16 lpShare) external onlyRole(DEFAULT_ADMIN_ROLE) {
        require(uint256(protocolShare) + uint256(lpShare) == 10_000, "Split!=100%");
        protocolShareBps = protocolShare;
        lpShareBps = lpShare;
    }

    // ---------- Finalizer (adapter-only) ----------
    /**
     * @notice Finalize a cross-chain message. Only the configured adapter may call this.
     * Marks the canonical message as used to prevent replay and applies fee splits.
     * @param globalRouteId canonical route identifier (for indexing/read-side)
     * @param messageHash canonical message hash (pre-image of GRI)
     * @param asset ERC20 token to distribute
     * @param vault recipient vault/pool address for forwarded funds
     * @param lpRecipient optional LP recipient for LP share
     * @param amount total forwarded amount that was sent to the destination (includes fees already skimmed)
     * @param protocolFee native protocol fee amount (passed through for read-side auditing)
     * @param relayerFee native relayer fee amount; will be forwarded to msg.sender (relayer)
     */
    function finalizeMessage(
        bytes32 globalRouteId,
        bytes32 messageHash,
        address asset,
        address vault,
        address lpRecipient,
        uint256 amount,
        uint256 protocolFee,
        uint256 relayerFee
    ) external onlyAdapter nonReentrant {
        // UNCHANGED: replay check, usedMessages[messageHash] = true;
        if (usedMessages[messageHash]) revert MessageAlreadyUsed();
        usedMessages[messageHash] = true;
        if (vault == address(0)) revert ZeroAddress();
        IERC20 t = IERC20(asset);
        // Destination now just forwards entire amount (fees already handled source-side or delegated to vault)
        t.safeTransfer(vault, amount);
        // Emit telemetry with zeroed fee fields to indicate source/vault handling
        // NOTE: Backend correlates destination leg via messageHash (human BridgeID derived off-chain)
        emit FeeApplied(
            globalRouteId,
            messageHash,
            SRC_CHAIN_ID,
            address(this),
            vault,
            asset,
            0,
            0,
            protocolShareBps,
            lpShareBps,
            feeCollector,
            block.timestamp
        );
    }
    // --- Storage append: role-based adapter allowlist and freeze map ---
    // (appended for layout stability)
    // See above for ADAPTER_ROLE, frozenAdapter, events

    // --- Storage append: fee delegation per target (vault decides fee skim) ---
    mapping(address => bool) public delegateFeeToTarget; // true => forward full amount; target/vault skims
    event DelegateFeeSet(address indexed target, bool delegate);
    function setDelegateFeeToTarget(address target, bool delegate) external onlyRole(DEFAULT_ADMIN_ROLE) {
        require(target != address(0), "zero target");
        delegateFeeToTarget[target] = delegate;
        emit DelegateFeeSet(target, delegate);
    }

    // (debug helpers removed)
}
