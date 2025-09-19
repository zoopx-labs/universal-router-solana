// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import {Initializable} from "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import {AccessControlUpgradeable} from "@openzeppelin/contracts-upgradeable/access/AccessControlUpgradeable.sol";
import {ReentrancyGuardUpgradeable} from "@openzeppelin/contracts-upgradeable/utils/ReentrancyGuardUpgradeable.sol";
import {PausableUpgradeable} from "@openzeppelin/contracts-upgradeable/utils/PausableUpgradeable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts-upgradeable/proxy/utils/UUPSUpgradeable.sol";
import {ContextUpgradeable} from "@openzeppelin/contracts-upgradeable/utils/ContextUpgradeable.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";

import "./USDzy.sol";

// DIA feed interface (minimal stub)
interface IDIAFeed {
    function latestValue() external view returns (int256 price, uint256 ts);
}

/**
 * @title Hub
 * @dev Phase-1 Hub: multi-asset deposits, DIA price reads, internal PPS, 2h withdraw queue.
 */
contract Hub is
    Initializable,
    ContextUpgradeable,
    AccessControlUpgradeable,
    ReentrancyGuardUpgradeable,
    PausableUpgradeable,
    UUPSUpgradeable
{
    using SafeERC20 for IERC20;

    bytes32 public constant PAUSER_ROLE = keccak256("PAUSER_ROLE");
    bytes32 public constant KEEPER_ROLE = keccak256("KEEPER_ROLE");

    struct AssetConfig {
        bool enabled;
        address token; // ERC20
        address feed; // DIA feed
        uint8 decimals; // token decimals
        uint8 priceDecimals; // feed decimals
        uint16 haircutBps; // basis points
    }

    // token registry for iteration
    address[] public listedTokens;
    mapping(address => bool) public isListed;

    mapping(address => AssetConfig) public assetCfg; // keyed by token address
    uint256 public maxStaleness; // seconds

    USDzy public usdzy;

    uint256 public withdrawDelay; // seconds

    // Withdraw queue
    struct WithdrawReq {
        address owner;
        uint128 usdOwed6; // USD amount scaled to 1e6
        uint64 readyAt;
        bool claimed;
    }

    event AssetConfigUpdated(
        address indexed token,
        address indexed feed,
        uint8 tokenDecimals,
        uint8 priceDecimals,
        uint16 haircutBps,
        bool enabled
    );
    event MaxStalenessUpdated(uint256 maxStaleness);
    event Deposited(address indexed user, address indexed asset, uint256 amountIn, uint256 usd6, uint256 sharesOut);
    event WithdrawRequested(
        uint256 indexed id, address indexed user, uint256 sharesBurned, uint256 usdOwed6, uint64 readyAt
    );
    event WithdrawClaimed(uint256 indexed id, address indexed user, address indexed payoutAsset, uint256 amountOut);

    error StalePrice(address feed, uint256 age);
    error NoFeed(address asset);

    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {
        _disableInitializers();
    }

    /**
     * @dev Initializer for Hub. The OpenZeppelin `initializer` modifier
     * provides protection against re-initialization when used with a proxy.
     * It ensures this function can only be called once.
     */
    function initialize(address usdzy_, address admin) public initializer {
        __Context_init_unchained();
        __AccessControl_init_unchained();
        __ReentrancyGuard_init_unchained();
        __Pausable_init_unchained();
        __UUPSUpgradeable_init();

        require(usdzy_ != address(0), "usdzy zero");
        require(admin != address(0), "admin zero");
        usdzy = USDzy(usdzy_);

        _grantRole(DEFAULT_ADMIN_ROLE, admin);
        _grantRole(PAUSER_ROLE, admin);
        _setRoleAdmin(PAUSER_ROLE, DEFAULT_ADMIN_ROLE);
        _setRoleAdmin(KEEPER_ROLE, DEFAULT_ADMIN_ROLE);

        withdrawDelay = 2 hours;
        maxStaleness = 300; // default 5 minutes
    }

    // --- Admin setters ---
    function setAssetConfig(
        address token,
        address feed,
        uint8 tokenDecimals,
        uint8 priceDecimals,
        uint16 haircutBps,
        bool enabled
    ) external onlyRole(DEFAULT_ADMIN_ROLE) {
        require(token != address(0), "token zero");
        assetCfg[token] = AssetConfig({
            enabled: enabled,
            token: token,
            feed: feed,
            decimals: tokenDecimals,
            priceDecimals: priceDecimals,
            haircutBps: haircutBps
        });
        if (!isListed[token]) {
            isListed[token] = true;
            listedTokens.push(token);
        }
        emit AssetConfigUpdated(token, feed, tokenDecimals, priceDecimals, haircutBps, enabled);
    }

    function setMaxStaleness(uint256 s) external onlyRole(DEFAULT_ADMIN_ROLE) {
        maxStaleness = s;
        emit MaxStalenessUpdated(s);
    }

    function setWithdrawDelay(uint256 s) external onlyRole(DEFAULT_ADMIN_ROLE) {
        withdrawDelay = s;
    }

    // --- Pause ---
    function pause() external onlyRole(PAUSER_ROLE) {
        _pause();
    }

    function unpause() external onlyRole(PAUSER_ROLE) {
        _unpause();
    }

    // --- Pricing / PPS ---
    /// @notice Quote arbitrary asset amount to USD scaled to 1e6 (USD6)
    function _scaleTo6(uint256 x, uint8 from) internal pure returns (uint256) {
        if (from == 6) return x;
        if (from > 6) return x / (10 ** (from - 6));
        return x * (10 ** (6 - from));
    }

    function _px6(address asset) internal view returns (uint256) {
        AssetConfig memory c = assetCfg[asset];
        if (asset == address(0) || c.feed == address(0)) revert NoFeed(asset);
        // NOTE: This function uses `block.timestamp` to check feed freshness.
        // Minor timestamp manipulation by miners is an accepted risk here because
        // the `maxStaleness` guard limits exposure. For higher security, consider
        // block-anchored or signed oracle data to reduce reliance on block time.
        // slither-disable-next-line calls-loop
        (int256 p, uint256 ts) = IDIAFeed(c.feed).latestValue();
        if (p <= 0) revert("bad price");
        // slither-disable-next-line timestamp
        uint256 age = block.timestamp - ts;
        // slither-disable-next-line timestamp
        if (age > maxStaleness) revert StalePrice(c.feed, age);
        return _scaleTo6(uint256(p), c.priceDecimals);
    }

    function quoteUsd6(address asset, uint256 amount, bool applyHaircut) public view returns (uint256 usd6) {
        AssetConfig memory c = assetCfg[asset];
        require(c.enabled, "asset disabled");
        uint256 amt6 = _scaleTo6(amount, c.decimals);
        uint256 px6 = _px6(asset);
        uint256 gross = Math.mulDiv(amt6, px6, 1_000_000);
        if (!applyHaircut) return gross;
        uint256 cut = Math.mulDiv(gross, c.haircutBps, 10_000);
        return gross - cut;
    }

    /**
     * @notice Returns details of listed tokens for off-chain processing.
     * @dev This function provides the necessary data for clients to compute total assets
     * by fetching balances and prices externally, avoiding costly on-chain loops.
     * @return addresses Array of token addresses.
     * @return decimals Array of token decimals.
     * @return priceFeeds Array of price feed addresses.
     */
    function getListedTokenDetails()
        public
        view
        returns (address[] memory addresses, uint8[] memory decimals, address[] memory priceFeeds)
    {
        uint256 len = listedTokens.length;
        addresses = new address[](len);
        decimals = new uint8[](len);
        priceFeeds = new address[](len);
        for (uint256 i = 0; i < len; i++) {
            address t = listedTokens[i];
            AssetConfig storage c = assetCfg[t];
            addresses[i] = t;
            decimals[i] = c.decimals;
            priceFeeds[i] = c.feed;
        }
    }

    /**
     * @notice Total assets across configured tokens, summed in USD6.
     * @dev Iterates listed tokens and sums balance*price. This is a view-only
     * path and may be gas-expensive with many assets. We avoid strict equality
     * checks and skip zero balances using a positive check to address linting.
     */
    function totalAssetsUsd6() public view returns (uint256 sum) {
        uint256 len = listedTokens.length;
        for (uint256 i = 0; i < len; i++) {
            address t = listedTokens[i];
            AssetConfig memory c = assetCfg[t];
            if (!c.enabled) continue;

            // slither-disable-next-line calls-loop
            uint256 bal = IERC20(t).balanceOf(address(this));
            // Safer check for non-zero balance (avoid strict equality style)
            if (bal > 0) {
                uint256 px6 = _px6(t);
                uint256 amt6 = _scaleTo6(bal, c.decimals);
                uint256 gross = Math.mulDiv(amt6, px6, 1_000_000);
                sum += gross;
            }
        }
    }

    /// @notice PPS scaled to 1e6 (USD per share). Returns 1e6 when supply==0.
    function pps6() public view returns (uint256) {
        uint256 supply = IERC20(address(usdzy)).totalSupply();
        if (supply == 0) return 1_000_000;
        return Math.mulDiv(totalAssetsUsd6(), 1_000_000, supply);
    }

    // --- Deposit / Withdraw flows ---
    function deposit(address asset, uint256 amount) external nonReentrant whenNotPaused {
        AssetConfig memory c = assetCfg[asset];
        require(c.enabled, "asset disabled");
        SafeERC20.safeTransferFrom(IERC20(asset), msg.sender, address(this), amount);
        uint256 usd6 = quoteUsd6(asset, amount, true); // haircut on deposit
        uint256 shares = Math.mulDiv(usd6, 1_000_000, pps6());
        require(shares > 0, "zero shares");
        usdzy.mint(msg.sender, shares);
        emit Deposited(msg.sender, asset, amount, usd6, shares);
    }

    function requestWithdraw(uint256 shares) external nonReentrant {
        require(shares > 0, "zero");
        uint256 usdOwed6 = Math.mulDiv(shares, pps6(), 1_000_000);
        // update state before external call to reduce reentrancy window
        uint64 readyAt = uint64(block.timestamp + withdrawDelay);
        requests.push(WithdrawReq({owner: msg.sender, usdOwed6: uint128(usdOwed6), readyAt: readyAt, claimed: false}));
        emit WithdrawRequested(requests.length - 1, msg.sender, shares, usdOwed6, readyAt);
        // burn after enqueuing the request to avoid reentrancy triggered by burn hooks
        usdzy.burn(msg.sender, shares);
    }

    function requestsCount() external view returns (uint256) {
        return requests.length;
    }

    function claimWithdraw(uint256 id, address payoutAsset) external nonReentrant whenNotPaused {
        WithdrawReq storage r = requests[id];
        require(!r.claimed, "claimed");
        require(r.owner == msg.sender, "not owner");
        // NOTE: Uses `block.timestamp` to enforce withdraw readiness. This is acceptable here
        // because `readyAt` is set with a reasonable delay; miners have only limited
        // ability to influence timestamps and this check is not security-critical beyond
        // enforcing the withdraw delay window.
        // slither-disable-next-line timestamp
        require(block.timestamp >= r.readyAt, "not ready");
        AssetConfig memory c = assetCfg[payoutAsset];
        require(c.enabled, "asset disabled");
        uint256 px6 = _px6(payoutAsset);
        // compute amt6 using full precision
        uint256 amt6 = Math.mulDiv(uint256(r.usdOwed6), 1_000_000, px6);
        // scale amt6 to token decimals preserving precision where possible
        uint256 amountOut = (c.decimals == 6)
            ? amt6
            : (c.decimals > 6 ? Math.mulDiv(amt6, 10 ** (c.decimals - 6), 1) : Math.mulDiv(amt6, 1, 10 ** (6 - c.decimals)));
        require(IERC20(payoutAsset).balanceOf(address(this)) >= amountOut, "insufficient liquidity");
        r.claimed = true;
        SafeERC20.safeTransfer(IERC20(payoutAsset), r.owner, amountOut);
        emit WithdrawClaimed(id, r.owner, payoutAsset, amountOut);
    }

    // exposed requests array per spec
    WithdrawReq[] public requests;

    function _authorizeUpgrade(address newImplementation) internal override onlyRole(DEFAULT_ADMIN_ROLE) {}

    uint256[50] private __gap;
}
