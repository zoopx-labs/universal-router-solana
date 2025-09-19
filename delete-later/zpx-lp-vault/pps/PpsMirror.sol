// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Initializable} from "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts-upgradeable/proxy/utils/UUPSUpgradeable.sol";
import {AccessControlUpgradeable} from "@openzeppelin/contracts-upgradeable/access/AccessControlUpgradeable.sol";
import {MessagingEndpointReceiver} from "../messaging/MessagingEndpointReceiver.sol";

import "../pps/IPpsSource.sol";

contract PpsMirror is
    Initializable,
    UUPSUpgradeable,
    AccessControlUpgradeable,
    MessagingEndpointReceiver,
    IPpsSource
{
    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {
        _disableInitializers();
    }

    bytes32 public constant POSTER_ROLE = keccak256("POSTER_ROLE");

    uint256 public pps6;
    uint64 public asOf;
    uint64 public remoteChainId;
    uint64 public maxStaleness;

    event PpsMirrored(uint256 pps6, uint64 asOf);

    // `initializer` prevents this function from being re-run and is the intended access
    // control for upgradeable contract initialization when used with OZ patterns.
    function initialize(address endpoint_, uint64 remoteChainId_) public initializer {
        __MessagingEndpointReceiver_init(endpoint_);
        __UUPSUpgradeable_init();
        // grant admin and poster to deployer so tests and deployments can grant further roles
        _grantRole(DEFAULT_ADMIN_ROLE, msg.sender);
        _grantRole(POSTER_ROLE, msg.sender);
        remoteChainId = remoteChainId_;
    }

    function post(uint256 newPps6, uint64 nowTs) external onlyRole(POSTER_ROLE) {
        pps6 = newPps6;
        asOf = nowTs;
        emit PpsMirrored(newPps6, nowTs);
    }

    function latestPps6() external view override returns (uint256, uint64) {
        return (pps6, asOf);
    }

    function setMaxStaleness(uint64 s) external onlyRole(DEFAULT_ADMIN_ROLE) {
        maxStaleness = s;
    }

    function _authorizeUpgrade(address) internal override onlyRole(DEFAULT_ADMIN_ROLE) {}

    // Storage gap for upgrade safety
    uint256[50] private __gap;
}
