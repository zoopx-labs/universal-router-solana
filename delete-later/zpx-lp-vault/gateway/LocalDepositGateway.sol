// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Initializable} from "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts-upgradeable/proxy/utils/UUPSUpgradeable.sol";
import {AccessControlUpgradeable} from "@openzeppelin/contracts-upgradeable/access/AccessControlUpgradeable.sol";
import {PausableUpgradeable} from "@openzeppelin/contracts-upgradeable/utils/PausableUpgradeable.sol";
import {ReentrancyGuardUpgradeable} from "@openzeppelin/contracts-upgradeable/utils/ReentrancyGuardUpgradeable.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";

import {IPpsSource} from "../pps/IPpsSource.sol";
import {IUSDzy} from "../usdzy/IUSDzy.sol";

interface IUSDzyRemoteMinter {
    function mintFromGateway(address to, uint256 shares) external;
}

interface IFeed {
    function latestAnswer() external view returns (int256);
    function latestTimestamp() external view returns (uint256);
}

interface ISpokeVault {
    function asset() external view returns (address);
}

contract LocalDepositGateway is
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

    using SafeERC20 for IERC20;

    bytes32 public constant GATEWAY_ADMIN = keccak256("GATEWAY_ADMIN");

    struct AssetCfg {
        bool enabled;
        address token;
        address feed;
        uint8 tokenDecimals;
        uint8 priceDecimals;
        uint16 haircutBps;
    }

    mapping(address => AssetCfg) public cfg;
    uint64 public maxStaleness;

    IUSDzyRemoteMinter public minter;
    IPpsSource public ppsMirror;
    ISpokeVault public spoke;

    event Deposited(address indexed user, address indexed asset, uint256 amount, uint256 usd6, uint256 shares);

    /**
     * @dev Initializer for LocalDepositGateway. The `initializer` modifier
     * from OpenZeppelin prevents this function from being re-run on a proxy.
     */
    function initialize(address minter_, address ppsMirror_, address spoke_, address admin_, uint64 maxStaleness_)
        public
        initializer
    {
        require(minter_ != address(0), "minter zero");
        require(ppsMirror_ != address(0), "ppsMirror zero");
        require(spoke_ != address(0), "spoke zero");
        require(admin_ != address(0), "admin zero");
        __AccessControl_init();
        __Pausable_init();
        __ReentrancyGuard_init();
        _grantRole(DEFAULT_ADMIN_ROLE, admin_);
        _grantRole(GATEWAY_ADMIN, admin_);
        minter = IUSDzyRemoteMinter(minter_);
        ppsMirror = IPpsSource(ppsMirror_);
        spoke = ISpokeVault(spoke_);
        maxStaleness = maxStaleness_;
    }

    function setAssetConfig(
        address token,
        address feed,
        uint8 tokenDecimals,
        uint8 priceDecimals,
        uint16 haircutBps,
        bool enabled
    ) external onlyRole(DEFAULT_ADMIN_ROLE) {
        cfg[token] = AssetCfg(enabled, token, feed, tokenDecimals, priceDecimals, haircutBps);
    }

    function setMaxStaleness(uint64 s) external onlyRole(DEFAULT_ADMIN_ROLE) {
        maxStaleness = s;
    }

    function deposit(address asset, uint256 amount) external nonReentrant whenNotPaused {
        AssetCfg memory c = cfg[asset];
        require(c.enabled, "ASSET_DISABLED");
        IERC20(asset).safeTransferFrom(msg.sender, address(this), amount);

        // read price
        IFeed feed = IFeed(c.feed);
        int256 price = feed.latestAnswer();
        require(price > 0, "BAD_PRICE");
        uint256 ts = feed.latestTimestamp();
        // slither-disable-next-line timestamp
        require(block.timestamp - ts <= maxStaleness, "PRICE_STALE");

        // Compute usd6 with precision using mulDiv where useful.
        // usd6 = amount * price * 10^(6 - priceDecimals) / 10^(tokenDecimals)
        uint256 usd6 = uint256(price) * amount;
        if (c.priceDecimals > 6) usd6 = usd6 / (10 ** (c.priceDecimals - 6));
        else if (c.priceDecimals < 6) usd6 = usd6 * (10 ** (6 - c.priceDecimals));
        if (c.tokenDecimals > 0) usd6 = Math.mulDiv(usd6, 1, 10 ** c.tokenDecimals);

        // apply haircut using mulDiv for precision
        uint256 haircut = Math.mulDiv(usd6, c.haircutBps, 10000);
        uint256 usd6After = usd6 >= haircut ? usd6 - haircut : 0;

        // read pps
        (uint256 pps6, uint64 ppsAsOf) = ppsMirror.latestPps6();
        // slither-disable-next-line timestamp
        require(block.timestamp - ppsAsOf <= maxStaleness, "PPS_STALE");
        require(pps6 > 0, "BAD_PPS");

        uint256 shares = Math.mulDiv(usd6After, 1e6, pps6);
        require(shares > 0, "ZERO_SHARES");

        // mint shares via minter
        minter.mintFromGateway(msg.sender, shares);

        // forward asset into spoke vault (assume spoke accepts tokens via transfer)
        IERC20(asset).safeTransfer(address(spoke), amount);

        emit Deposited(msg.sender, asset, amount, usd6After, shares);
    }

    function _authorizeUpgrade(address) internal override onlyRole(DEFAULT_ADMIN_ROLE) {}

    // Storage gap for upgrade safety
    uint256[50] private __gap;
}
