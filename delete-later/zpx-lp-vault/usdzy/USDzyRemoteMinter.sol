// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Initializable} from "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts-upgradeable/proxy/utils/UUPSUpgradeable.sol";
import {AccessControlUpgradeable} from "@openzeppelin/contracts-upgradeable/access/AccessControlUpgradeable.sol";
import {PausableUpgradeable} from "@openzeppelin/contracts-upgradeable/utils/PausableUpgradeable.sol";
import {ReentrancyGuardUpgradeable} from "@openzeppelin/contracts-upgradeable/utils/ReentrancyGuardUpgradeable.sol";
import {MessagingEndpointReceiver} from "../messaging/MessagingEndpointReceiver.sol";

interface IUSDzy {
    function mint(address to, uint256 amount) external;
}

contract USDzyRemoteMinter is
    Initializable,
    UUPSUpgradeable,
    AccessControlUpgradeable,
    PausableUpgradeable,
    ReentrancyGuardUpgradeable,
    MessagingEndpointReceiver
{
    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {
        _disableInitializers();
    }

    address public usdzy;
    bytes32 public constant GATEWAY_ROLE = keccak256("GATEWAY_ROLE");

    event GatewayMinted(address indexed to, uint256 shares);

    function initialize(address usdzy_, address admin_) public initializer {
        // initialize with a non-zero owner (admin) to satisfy OwnableUpgradeable
        __MessagingEndpointReceiver_init(admin_);
        __UUPSUpgradeable_init();

        require(usdzy_ != address(0), "usdzy=0");
        require(admin_ != address(0), "admin=0");

        usdzy = usdzy_;
        _grantRole(DEFAULT_ADMIN_ROLE, admin_);
    }

    function mintFromGateway(address to, uint256 shares) external onlyRole(GATEWAY_ROLE) whenNotPaused nonReentrant {
        IUSDzy(usdzy).mint(to, shares);
        emit GatewayMinted(to, shares);
    }

    function onMessage(uint64 srcChainId, address srcAddr, bytes calldata payload, uint64 nonce)
        external
        nonReentrant
    {
        _verifyAndMark(srcChainId, srcAddr, payload, nonce);
        (address to, uint256 amount) = abi.decode(payload, (address, uint256));
        IUSDzy(usdzy).mint(to, amount);
    }

    function _authorizeUpgrade(address) internal override onlyRole(DEFAULT_ADMIN_ROLE) {}

    // Storage gap for upgrade safety
    uint256[50] private __gap;
}
