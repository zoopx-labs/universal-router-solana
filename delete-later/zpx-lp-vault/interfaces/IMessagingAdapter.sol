// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

interface IMessagingAdapter {
    event MessageSent(uint64 dstChainId, address indexed dst, bytes payload, uint64 nonce);
    event MessageReceived(uint64 srcChainId, address indexed src, bytes payload, uint64 nonce);

    function send(uint64 dstChainId, address dst, bytes calldata payload) external returns (uint64 nonce);
}
