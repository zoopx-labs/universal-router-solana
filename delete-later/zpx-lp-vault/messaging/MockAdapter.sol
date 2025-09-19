// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {IMessagingAdapter} from "../interfaces/IMessagingAdapter.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

contract MockAdapter is IMessagingAdapter, ReentrancyGuard {
    address public immutable owner;
    uint64 public nonce;

    event EndpointSet(uint64 chainId, address endpoint, bool allowed);

    mapping(uint64 => mapping(address => bool)) public allowedEndpoint;

    constructor() {
        owner = msg.sender;
    }

    function setEndpoint(uint64 chainId, address endpoint, bool allowed) external {
        require(msg.sender == owner, "NOT_OWNER");
        allowedEndpoint[chainId][endpoint] = allowed;
        emit EndpointSet(chainId, endpoint, allowed);
    }

    function send(uint64 dstChainId, address dst, bytes calldata payload) external nonReentrant returns (uint64) {
        require(dst != address(0), "DST=0");
        nonce++;
        emit MessageSent(dstChainId, dst, payload, nonce);
        // In tests, if dst is a contract with onMessage, call it directly to simulate delivery
        // adapter forwards the original sender as the srcAddr in the message params
        (bool ok,) = dst.call(
            abi.encodeWithSignature("onMessage(uint64,address,bytes,uint64)", dstChainId, msg.sender, payload, nonce)
        );
        if (ok) emit MessageReceived(dstChainId, dst, payload, nonce);
        return nonce;
    }
}
