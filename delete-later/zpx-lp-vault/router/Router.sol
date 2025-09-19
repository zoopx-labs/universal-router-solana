// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Initializable} from "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts-upgradeable/proxy/utils/UUPSUpgradeable.sol";
import {AccessControlUpgradeable} from "@openzeppelin/contracts-upgradeable/access/AccessControlUpgradeable.sol";
import {PausableUpgradeable} from "@openzeppelin/contracts-upgradeable/utils/PausableUpgradeable.sol";
import {ReentrancyGuardUpgradeable} from "@openzeppelin/contracts-upgradeable/utils/ReentrancyGuardUpgradeable.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";

import {ISpokeVault} from "../interfaces/ISpokeVault.sol";
import {IMessagingAdapter} from "../interfaces/IMessagingAdapter.sol";
import {IMessagingEndpoint} from "../messaging/IMessagingEndpoint.sol";

contract Router is
    Initializable,
    UUPSUpgradeable,
    AccessControlUpgradeable,
    PausableUpgradeable,
    ReentrancyGuardUpgradeable
{
    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {
        _disableInitializers();
    }

    bytes32 public constant KEEPER_ROLE = keccak256("KEEPER_ROLE");
    bytes32 public constant RELAYER_ROLE = keccak256("RELAYER_ROLE");
    bytes32 public constant PAUSER_ROLE = keccak256("PAUSER_ROLE");
    bytes32 public constant REBALANCER_ROLE = keccak256("REBALANCER_ROLE");

    using SafeERC20 for IERC20;

    ISpokeVault public vault;
    IMessagingAdapter public adapter;
    IMessagingEndpoint public messagingEndpoint;
    address public feeCollector;
    uint64 public lastSendNonce;

    // fees are in basis points (10000 = 100%)
    uint16 public protocolFeeBps; // capped at 5 bps (0.05%)
    uint16 public relayerFeeBps; // no hard cap (ops-defined)
    uint16 public protocolShareBps; // default 2500 (25%)
    uint16 public lpShareBps; // default 7500 (75%)

    event ProtocolFeeUpdated(uint16 bps);
    event RelayerFeeUpdated(uint16 bps);
    event FeeSplitUpdated(uint16 protocolShareBps, uint16 lpShareBps);
    event FeeCollectorUpdated(address feeCollector);
    event FeeApplied(
        address asset,
        uint256 grossAmount,
        uint256 relayerFee,
        uint256 protocolFee,
        uint256 protocolToTreasury,
        uint256 protocolToLPs,
        address to,
        address relayerAddr,
        address feeCollector
    );

    struct Day {
        uint64 day;
        uint256 tvl;
    }

    Day[7] public daysBuf;
    uint8 public idx;
    uint64 public lastSnapDay;

    uint64 public lastRebalanceAt;

    event RebalanceSuggested(uint256 tvl, uint256 avg7d, uint16 healthBps, uint64 lastRebalanceAt);
    event FillExecuted(address to, uint256 amount);
    event Repaid(uint256 amount, uint256 debtAfter);

    /**
     * @dev Initializer. The OpenZeppelin `initializer` modifier ensures this
     * function can only be called once when used with a proxy deployment.
     */
    function initialize(address vault_, address messagingEndpoint_, address admin, address feeCollector_)
        public
        initializer
    {
        __Context_init_unchained();
        __AccessControl_init_unchained();
        __ReentrancyGuard_init_unchained();
        __Pausable_init_unchained();
        __UUPSUpgradeable_init();
        require(vault_ != address(0), "vault=0");
        require(messagingEndpoint_ != address(0), "endpoint=0");
        // allow feeCollector_ to be zero only if protocolFeeBps will remain zero until set non-zero
        require(feeCollector_ != address(0), "feeCollector=0");

        vault = ISpokeVault(vault_);
        messagingEndpoint = IMessagingEndpoint(messagingEndpoint_);
        feeCollector = feeCollector_;

        _grantRole(DEFAULT_ADMIN_ROLE, admin);
        _grantRole(PAUSER_ROLE, admin);
        _grantRole(RELAYER_ROLE, admin);
        _grantRole(REBALANCER_ROLE, admin);

        _setRoleAdmin(PAUSER_ROLE, DEFAULT_ADMIN_ROLE);
        _setRoleAdmin(RELAYER_ROLE, DEFAULT_ADMIN_ROLE);
        _setRoleAdmin(REBALANCER_ROLE, DEFAULT_ADMIN_ROLE);
    }

    // --- Fee setters ---
    function setProtocolFeeBps(uint16 bps) external onlyRole(DEFAULT_ADMIN_ROLE) {
        require(bps <= 5, "ProtocolFee>5bps");
        protocolFeeBps = bps;
        emit ProtocolFeeUpdated(bps);
    }

    function setRelayerFeeBps(uint16 bps) external onlyRole(DEFAULT_ADMIN_ROLE) {
        relayerFeeBps = bps;
        emit RelayerFeeUpdated(bps);
    }

    function setFeeSplit(uint16 protocolShare, uint16 lpShare) external onlyRole(DEFAULT_ADMIN_ROLE) {
        require(uint256(protocolShare) + uint256(lpShare) == 10000, "Split!=100%");
        protocolShareBps = protocolShare;
        lpShareBps = lpShare;
        emit FeeSplitUpdated(protocolShare, lpShare);
    }

    function setFeeCollector(address collector) external onlyRole(DEFAULT_ADMIN_ROLE) {
        // allow clearing collector only if protocol fee is zero
        require(collector != address(0) || protocolFeeBps == 0, "FeeCollector=0");
        feeCollector = collector;
        emit FeeCollectorUpdated(collector);
    }

    function tvl() public view returns (uint256) {
        return vault.totalAssets();
    }

    function available() public view returns (uint256) {
        return IERC20(vault.asset()).balanceOf(address(vault));
    }

    function pokeTvlSnapshot() public {
        // slither-disable-next-line timestamp
        uint64 day = uint64(block.timestamp / 1 days);
        // if we've already recorded for this day or a later day, skip
        if (day <= lastSnapDay) return; // idempotent and resilient to clock skew
        lastSnapDay = day;
        idx = uint8((uint256(idx) + 1) % 7);
        daysBuf[idx] = Day({day: day, tvl: tvl()});
    }

    function avg7d() public view returns (uint256) {
        uint256 s = 0;
        for (uint256 i = 0; i < 7; i++) {
            s += daysBuf[i].tvl;
        }
        return s / 7;
    }

    function healthBps() public view returns (uint16 h) {
        uint256 a = available();
        uint256 t = tvl();
        if (t == 0) return 65535; // max health
        uint256 h_ = (a * 65535) / t;
        if (h_ > 65535) return 65535;
        h = uint16(h_);
    }

    function needsRebalance() public view returns (bool) {
        if (healthBps() < 4000) return true;
        // use an explicit delta comparison to avoid underflow and reduce reliance on exact equality
        // slither-disable-next-line timestamp
        if (block.timestamp >= lastRebalanceAt + 1 days) return true;
        return false;
    }

    function rebalance(uint64 dstChainId, address hubAddr) external onlyRole(KEEPER_ROLE) nonReentrant whenNotPaused {
        require(needsRebalance(), "NO_REBALANCE");
        uint256 cur = tvl();
        uint256 avg = avg7d();
        uint16 h = healthBps();
        emit RebalanceSuggested(cur, avg, h, lastRebalanceAt);
        bytes memory payload = abi.encode(dstChainId, address(vault), vault.totalAssets(), cur, h);
        // record timestamp before external adapter call to reduce reentrancy window
        lastRebalanceAt = uint64(block.timestamp);
        // send message and record nonce for traceability
        lastSendNonce = adapter.send(dstChainId, hubAddr, payload);
    }

    function fill(address to, uint256 amount) external onlyRole(RELAYER_ROLE) nonReentrant whenNotPaused {
        // compute fees
        uint256 protocolFee = Math.mulDiv(uint256(amount), uint256(protocolFeeBps), 10000);
        uint256 relayerFee = Math.mulDiv(uint256(amount), uint256(relayerFeeBps), 10000);

        uint256 net;
        if (protocolFee + relayerFee >= amount) {
            // protect against zero/negative net
            net = 0;
        } else {
            net = amount - protocolFee - relayerFee;
        }

        uint256 toTreasury = 0;
        uint256 toLPs = 0;

        // handle protocol split and transfers by borrowing from vault to recipients
        if (protocolFee > 0) {
            require(feeCollector != address(0), "FeeCollector=0");
            toTreasury = Math.mulDiv(protocolFee, uint256(protocolShareBps), 10000);
            toLPs = protocolFee - toTreasury;
            // send treasury its share by borrowing from the vault (capture returned debt for clarity)
            if (toTreasury > 0) {
                uint256 _debt = vault.borrow(toTreasury, feeCollector);
                // use _debt to avoid ignoring return value
                if (_debt == type(uint256).max) revert();
            }
            // toLPs remains in the vault (no action) to benefit LPs
        }

        // pay relayer from vault via borrow
        if (relayerFee > 0) {
            uint256 _debt = vault.borrow(relayerFee, msg.sender);
            if (_debt == type(uint256).max) revert();
        }

        // deliver net to user from vault
        if (net > 0) {
            uint256 _debt = vault.borrow(net, to);
            if (_debt == type(uint256).max) revert();
        }

        // emit enriched fee application event
        emit FeeApplied(
            address(vault.asset()), amount, relayerFee, protocolFee, toTreasury, toLPs, to, msg.sender, feeCollector
        );
        emit FillExecuted(to, amount);
    }

    function repay(uint256 amount) external nonReentrant whenNotPaused {
        IERC20(vault.asset()).safeTransferFrom(msg.sender, address(vault), amount);
        uint256 newDebt = vault.repay(amount);
        emit Repaid(amount, newDebt);
    }

    function setAdapter(address a) external onlyRole(DEFAULT_ADMIN_ROLE) {
        adapter = IMessagingAdapter(a);
    }

    function pause() external onlyRole(PAUSER_ROLE) {
        _pause();
    }

    function unpause() external onlyRole(PAUSER_ROLE) {
        _unpause();
    }

    function _authorizeUpgrade(address) internal override onlyRole(DEFAULT_ADMIN_ROLE) {}

    // Storage gap for upgrade safety
    uint256[50] private __gap;
}
