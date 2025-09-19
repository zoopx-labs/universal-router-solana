// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {AccessControlUpgradeable} from "@openzeppelin/contracts-upgradeable/access/AccessControlUpgradeable.sol";
import {Initializable} from "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import {ReentrancyGuardUpgradeable} from "@openzeppelin/contracts-upgradeable/utils/ReentrancyGuardUpgradeable.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";

import {SpokeVault} from "../spoke/SpokeVault.sol";
import {Router} from "../router/Router.sol";

contract Factory is Initializable, AccessControlUpgradeable, ReentrancyGuardUpgradeable {
    event SpokeDeployed(uint64 chainId, address asset, address vault, address router);

    // cached implementations (can be rotated by admin)
    address public spokeVaultImpl;
    address public routerImpl;

    event ImplementationUpdated(bytes32 what, address impl);

    function initialize(address admin) public initializer {
        require(admin != address(0), "admin zero");
        __AccessControl_init();
        __ReentrancyGuard_init();
        _grantRole(DEFAULT_ADMIN_ROLE, admin);
    }

    function setSpokeVaultImpl(address impl) public onlyRole(DEFAULT_ADMIN_ROLE) {
        require(impl != address(0), "impl zero");
        spokeVaultImpl = impl;
        emit ImplementationUpdated("SpokeVault", impl);
    }

    function setRouterImpl(address impl) public onlyRole(DEFAULT_ADMIN_ROLE) {
        require(impl != address(0), "impl zero");
        routerImpl = impl;
        emit ImplementationUpdated("Router", impl);
    }

    function deploySpoke(
        uint64 chainId,
        address asset,
        string memory name,
        string memory symbol,
        address admin,
        address routerAdmin,
        address adapter,
        address feeCollector
    ) external onlyRole(DEFAULT_ADMIN_ROLE) nonReentrant returns (address vault, address router) {
        // determine implementations to use (cached if set, otherwise deploy new impls)
        address implVAddr = spokeVaultImpl;
        if (implVAddr == address(0)) {
            SpokeVault implV = new SpokeVault();
            implVAddr = address(implV);
            // cache the implementation so future deploys reuse it
            setSpokeVaultImpl(implVAddr);
        }

        address implRAddr = routerImpl;
        if (implRAddr == address(0)) {
            Router implR = new Router();
            implRAddr = address(implR);
            // cache the router impl as well
            setRouterImpl(implRAddr);
        }

        // prepare initializers and deploy proxies
        // initialize the vault with this Factory as temporary admin so we can wire roles,
        // then transfer admin/pauser to the requested admin afterwards
        bytes memory initV = abi.encodeCall(SpokeVault.initialize, (asset, name, symbol, address(this)));
        vault = address(new ERC1967Proxy(implVAddr, initV));

        // initialize router with this Factory as temporary admin so we can wire roles and pause it
        // Note: Router.initialize takes messagingEndpoint, not adapter. We'll set adapter explicitly below.
        bytes memory initR = abi.encodeCall(Router.initialize, (vault, adapter, address(this), feeCollector));
        router = address(new ERC1967Proxy(implRAddr, initR));

        // grant BORROWER_ROLE on vault to router proxy (Factory is temporary admin so this will succeed)
        SpokeVault(vault).grantRole(SpokeVault(vault).BORROWER_ROLE(), router);

        // set adapter on the router now that it's deployed (Factory is temporary admin)
        Router(router).setAdapter(adapter);

        // keep vault & router paused by default for safety (we are temporary admin)
        SpokeVault(vault).pause();
        Router(router).pause();

        // Transfer admin and pauser roles to the requested admin, then renounce Factory roles
        if (admin != address(0) && admin != address(this)) {
            // grant DEFAULT_ADMIN_ROLE and PAUSER_ROLE to the desired admin on the vault
            SpokeVault(vault).grantRole(SpokeVault(vault).DEFAULT_ADMIN_ROLE(), admin);
            SpokeVault(vault).grantRole(SpokeVault(vault).PAUSER_ROLE(), admin);

            // renounce Factory's roles so admin is the sole admin/pauser
            SpokeVault(vault).renounceRole(SpokeVault(vault).PAUSER_ROLE(), address(this));
            SpokeVault(vault).renounceRole(SpokeVault(vault).DEFAULT_ADMIN_ROLE(), address(this));
        }

        if (routerAdmin != address(0) && routerAdmin != address(this)) {
            // grant DEFAULT_ADMIN_ROLE and PAUSER_ROLE to the desired router admin
            Router(router).grantRole(Router(router).DEFAULT_ADMIN_ROLE(), routerAdmin);
            Router(router).grantRole(Router(router).PAUSER_ROLE(), routerAdmin);

            // renounce Factory's roles on router
            Router(router).renounceRole(Router(router).PAUSER_ROLE(), address(this));
            Router(router).renounceRole(Router(router).DEFAULT_ADMIN_ROLE(), address(this));
        }

        emit SpokeDeployed(chainId, asset, vault, router);
        return (vault, router);
    }
}
