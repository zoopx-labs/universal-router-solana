// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {OwnableUpgradeable} from "@openzeppelin/contracts-upgradeable/access/OwnableUpgradeable.sol";
import {Initializable} from "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import {IMessagingAdapter} from "../interfaces/IMessagingAdapter.sol";

contract MessagingEndpointReceiver is Initializable, OwnableUpgradeable {
    // mapping of allowed source chain => allowed source address
    mapping(uint64 => mapping(address => bool)) public allowedEndpoint;
    mapping(bytes32 => bool) public used;
    IMessagingAdapter public adapter;

    event EndpointSet(uint64 chainId, address endpoint, bool allowed);

    modifier onlyAllowed(uint64 srcChainId, address src) {
        require(allowedEndpoint[srcChainId][src], "BAD_ENDPOINT");
        _;
    }

    modifier onlyAdapter() {
        require(address(adapter) != address(0), "NO_ADAPTER");
        require(msg.sender == address(adapter), "NOT_ADAPTER");
        _;
    }

    // Allow owner or the configured adapter to register allowed source endpoints on this receiver.
    function setEndpoint(uint64 chainId, address endpoint, bool allowed) public {
        // owner or adapter may call to register allowed source addresses
        if (msg.sender != owner()) {
            require(address(adapter) != address(0) && msg.sender == address(adapter), "NOT_AUTHORIZED");
        }
        allowedEndpoint[chainId][endpoint] = allowed;
        emit EndpointSet(chainId, endpoint, allowed);
    }

    function setAdapter(address a) external onlyOwner {
        adapter = IMessagingAdapter(a);
    }

    function __MessagingEndpointReceiver_init(address owner_) internal initializer {
        __Ownable_init(owner_);
    }

    function _markUsed(bytes32 key) internal returns (bool) {
        if (used[key]) return false;
        used[key] = true;
        return true;
    }

    /**
     * @dev Verify that the caller is the declared source endpoint and that the endpoint is allowed.
     * Also checks and marks the (srcChainId, srcAddr, payload, nonce) key to prevent replays.
     * Reverts on bad endpoint, bad caller, or replay.
     */
    function _verifyAndMark(uint64 srcChainId, address srcAddr, bytes calldata payload, uint64 nonce)
        internal
        returns (bytes32)
    {
        if (address(adapter) == address(0)) {
            // legacy behavior: adapter not set, require caller == srcAddr
            require(allowedEndpoint[srcChainId][srcAddr], "BAD_ENDPOINT");
            require(msg.sender == srcAddr, "BAD_SENDER");
        } else {
            // production behavior: only adapter calls in, srcAddr must be allowed
            require(msg.sender == address(adapter), "NOT_ADAPTER");
            require(allowedEndpoint[srcChainId][srcAddr], "BAD_ENDPOINT");
        }
        bytes32 key = keccak256(abi.encodePacked(srcChainId, srcAddr, payload, nonce));
        require(_markUsed(key), "REPLAY");
        return key;
    }

    // storage gap for upgrade safety
    uint256[50] private __gap;
}
