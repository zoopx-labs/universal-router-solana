// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Initializable} from "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts-upgradeable/proxy/utils/UUPSUpgradeable.sol";
import {AccessControlUpgradeable} from "@openzeppelin/contracts-upgradeable/access/AccessControlUpgradeable.sol";

import "../policy/IPolicySource.sol";

contract PolicyBeacon is Initializable, UUPSUpgradeable, AccessControlUpgradeable, IPolicySource {
    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {
        _disableInitializers();
    }

    bytes32 public constant POSTER_ROLE = keccak256("POSTER_ROLE");

    struct Record {
        uint64 chainId;
        address spokeVault;
        address router;
        uint256 tvlUsd6;
        uint256 ma7Usd6;
        uint16 coverageBps;
        IPolicySource.State state;
        uint64 asOf;
        bytes32 ref;
    }

    mapping(address => Record) public latest;

    event PolicyEvaluated(
        uint64 chainId,
        address indexed spokeVault,
        address indexed router,
        uint256 tvlUsd6,
        uint256 ma7Usd6,
        uint16 coverageBps,
        IPolicySource.State state,
        uint64 asOf,
        bytes32 ref,
        bool persisted
    );

    // `initializer` protects this setup function from being called more than once.
    // Ensure deployment follows OZ upgradeable patterns (proxy + initializer) so this is effective.
    function initialize(address admin) public initializer {
        __Context_init_unchained();
        __AccessControl_init_unchained();
        __UUPSUpgradeable_init();
        _grantRole(DEFAULT_ADMIN_ROLE, admin);
    }

    function post(
        uint64 chainId,
        address spokeVault,
        address router,
        uint256 tvlUsd6,
        uint256 ma7Usd6,
        uint16 coverageBps,
        IPolicySource.State state,
        uint64 asOf,
        bytes32 ref,
        bool persist
    ) external onlyRole(POSTER_ROLE) {
        if (persist) {
            latest[spokeVault] = Record(chainId, spokeVault, router, tvlUsd6, ma7Usd6, coverageBps, state, asOf, ref);
        }
        emit PolicyEvaluated(chainId, spokeVault, router, tvlUsd6, ma7Usd6, coverageBps, state, asOf, ref, persist);
    }

    function latestOf(address spokeVault)
        external
        view
        override
        returns (uint256 tvlUsd6, uint256 ma7Usd6, uint16 coverageBps, IPolicySource.State state, uint64 asOf)
    {
        Record memory r = latest[spokeVault];
        require(r.spokeVault != address(0), "NOT_FOUND");
        return (r.tvlUsd6, r.ma7Usd6, r.coverageBps, r.state, r.asOf);
    }

    function _authorizeUpgrade(address) internal override onlyRole(DEFAULT_ADMIN_ROLE) {}

    // storage gap for upgrade safety
    uint256[50] private __gap;
}
