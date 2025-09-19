// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import {Initializable} from "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import {ERC20Upgradeable} from "@openzeppelin/contracts-upgradeable/token/ERC20/ERC20Upgradeable.sol";
import {ERC20PermitUpgradeable} from
    "@openzeppelin/contracts-upgradeable/token/ERC20/extensions/ERC20PermitUpgradeable.sol";
import {ERC20BurnableUpgradeable} from
    "@openzeppelin/contracts-upgradeable/token/ERC20/extensions/ERC20BurnableUpgradeable.sol";
import {AccessControlUpgradeable} from "@openzeppelin/contracts-upgradeable/access/AccessControlUpgradeable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts-upgradeable/proxy/utils/UUPSUpgradeable.sol";

/**
 * @title ZPXArb
 * @dev UUPS upgradeable mintable ERC20 with permit, burnable (ERC20BurnableUpgradeable) and AccessControl.
 */
contract ZPXArb is
    Initializable,
    ERC20Upgradeable,
    ERC20PermitUpgradeable,
    ERC20BurnableUpgradeable,
    AccessControlUpgradeable,
    UUPSUpgradeable
{
    bytes32 public constant MINTER_ROLE = keccak256("MINTER_ROLE");

    event Minted(address indexed to, uint256 amount);

    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {}

    // `initializer` restricts this setup function to a single call when used with a proxy.
    function initialize(string memory name_, string memory symbol_, address admin) public initializer {
        __ERC20_init(name_, symbol_);
        __ERC20Permit_init(name_);
        __ERC20Burnable_init();
        __AccessControl_init_unchained();
        __UUPSUpgradeable_init();

        _grantRole(DEFAULT_ADMIN_ROLE, admin);
        _setRoleAdmin(MINTER_ROLE, DEFAULT_ADMIN_ROLE);
    }

    function mint(address to, uint256 amount) external onlyRole(MINTER_ROLE) {
        _mint(to, amount);
        emit Minted(to, amount);
    }

    function decimals() public pure override returns (uint8) {
        return 18;
    }

    /// @notice Burn from owner using EIP-2612 permit signature issued by owner
    function burnFromWithPermit(address owner, uint256 amount, uint256 deadline, uint8 v, bytes32 r, bytes32 s)
        external
    {
        // permit the caller (msg.sender) to spend on behalf of owner
        permit(owner, msg.sender, amount, deadline, v, r, s);
        super.burnFrom(owner, amount);
    }

    function _authorizeUpgrade(address) internal override onlyRole(DEFAULT_ADMIN_ROLE) {}

    // gap for future variables
    uint256[50] private __gap;
}
