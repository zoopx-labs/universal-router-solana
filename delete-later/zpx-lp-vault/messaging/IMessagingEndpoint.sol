// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

interface IMessagingEndpoint {
    function send(uint64 dstChainId, address to, bytes calldata payload) external returns (uint64 nonce);
}
