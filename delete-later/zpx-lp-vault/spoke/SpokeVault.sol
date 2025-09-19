// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Initializable} from "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts-upgradeable/proxy/utils/UUPSUpgradeable.sol";
import {ERC20Upgradeable} from "@openzeppelin/contracts-upgradeable/token/ERC20/ERC20Upgradeable.sol";
import {ERC20PermitUpgradeable} from
    "@openzeppelin/contracts-upgradeable/token/ERC20/extensions/ERC20PermitUpgradeable.sol";
import {ERC4626Upgradeable} from "@openzeppelin/contracts-upgradeable/token/ERC20/extensions/ERC4626Upgradeable.sol";
import {PausableUpgradeable} from "@openzeppelin/contracts-upgradeable/utils/PausableUpgradeable.sol";
import {ReentrancyGuardUpgradeable} from "@openzeppelin/contracts-upgradeable/utils/ReentrancyGuardUpgradeable.sol";
import {AccessControlUpgradeable} from "@openzeppelin/contracts-upgradeable/access/AccessControlUpgradeable.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";

import {ISpokeVault} from "../interfaces/ISpokeVault.sol";

contract SpokeVault is
    Initializable,
    UUPSUpgradeable,
    ERC20Upgradeable,
    ERC20PermitUpgradeable,
    ERC4626Upgradeable,
    PausableUpgradeable,
    ReentrancyGuardUpgradeable,
    AccessControlUpgradeable,
    ISpokeVault
{
    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {
        _disableInitializers();
    }

    using SafeERC20 for IERC20;

    bytes32 public constant PAUSER_ROLE = keccak256("PAUSER_ROLE");
    bytes32 public constant BORROWER_ROLE = keccak256("BORROWER_ROLE");

    uint256 public debt;
    uint256 public borrowCap;
    uint16 public maxUtilizationBps;

    event Borrowed(uint256 amount, address to, uint256 debtAfter);
    event Repaid(uint256 amount, uint256 debtAfter);
    event IdleSent(address to, uint256 amount);
    event BorrowCapUpdated(uint256 newCap);
    event MaxUtilizationUpdated(uint16 newMax);

    error BORROW_OVER_CAP();
    error BORROW_OVER_UTIL();
    error LP_DISABLED();

    // Resolve multiple inheritance name clashes from ERC4626 and interfaces
    function asset() public view override(ERC4626Upgradeable, ISpokeVault) returns (address) {
        return ERC4626Upgradeable.asset();
    }

    function decimals() public view override(ERC20Upgradeable, ERC4626Upgradeable) returns (uint8) {
        return ERC20Upgradeable.decimals();
    }

    function totalAssets() public view override(ERC4626Upgradeable, ISpokeVault) returns (uint256) {
        return ERC4626Upgradeable.totalAssets();
    }

    // The `initializer` modifier ensures this function can only be executed once when
    // used correctly with a proxy. Keep this pattern to prevent unprotected initialization.
    function initialize(address asset_, string memory name_, string memory symbol_, address admin_)
        public
        initializer
    {
        require(asset_ != address(0), "asset zero");
        require(admin_ != address(0), "admin zero");
        __ERC20_init(name_, symbol_);
        __ERC20Permit_init(name_);
        __ERC4626_init(IERC20(asset_));
        __Pausable_init();
        __ReentrancyGuard_init();
        __AccessControl_init();
        _grantRole(DEFAULT_ADMIN_ROLE, admin_);
        _grantRole(PAUSER_ROLE, admin_);
        // BORROWER_ROLE left for Router to claim
        borrowCap = type(uint256).max;
        maxUtilizationBps = 9000;
    }

    // disable ERC4626 deposits/withdrawals on spokes
    function deposit(uint256, address) public pure override returns (uint256) {
        revert LP_DISABLED();
    }

    function mint(uint256, address) public pure override returns (uint256) {
        revert LP_DISABLED();
    }

    function withdraw(uint256, address, address) public pure override returns (uint256) {
        revert LP_DISABLED();
    }

    function redeem(uint256, address, address) public pure override returns (uint256) {
        revert LP_DISABLED();
    }

    function maxBorrow() external view returns (uint256) {
        return borrowCap;
    }

    function borrow(uint256 amt, address to)
        external
        onlyRole(BORROWER_ROLE)
        whenNotPaused
        nonReentrant
        returns (uint256)
    {
        uint256 newDebt = debt + amt;
        if (newDebt > borrowCap) revert BORROW_OVER_CAP();
        uint256 ta = totalAssets();
        if (ta > 0) {
            uint256 util = (newDebt * 10000) / ta;
            if (util > maxUtilizationBps) revert BORROW_OVER_UTIL();
        }
        debt = newDebt;
        IERC20(asset()).safeTransfer(to, amt);
        emit Borrowed(amt, to, debt);
        return debt;
    }

    function repay(uint256 amt) external whenNotPaused nonReentrant returns (uint256) {
        IERC20(asset()).safeTransferFrom(msg.sender, address(this), amt);
        if (amt > debt) debt = 0;
        else debt -= amt;
        emit Repaid(amt, debt);
        return debt;
    }

    function idleLiquidity() external view returns (uint256) {
        uint256 bal = IERC20(asset()).balanceOf(address(this));
        return bal >= debt ? bal - debt : 0;
    }

    function sendIdle(address to, uint256 amount) external onlyRole(DEFAULT_ADMIN_ROLE) whenNotPaused {
        uint256 idle = this.idleLiquidity();
        require(amount <= idle, "OVER_IDLE");
        IERC20(asset()).safeTransfer(to, amount);
        emit IdleSent(to, amount);
    }

    function utilizationBps() external view returns (uint16) {
        uint256 ta = totalAssets();
        // Avoid strict equality; use positive guard
        if (ta > 0) {
            return uint16((debt * 10000) / ta);
        }
        return 0;
    }

    function setBorrowCap(uint256 newCap) external onlyRole(DEFAULT_ADMIN_ROLE) {
        borrowCap = newCap;
        emit BorrowCapUpdated(newCap);
    }

    function setMaxUtilizationBps(uint16 bps) external onlyRole(DEFAULT_ADMIN_ROLE) {
        maxUtilizationBps = bps;
        emit MaxUtilizationUpdated(bps);
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
