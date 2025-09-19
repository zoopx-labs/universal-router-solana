// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/**
 * @title Hashing
 * @notice Canonical hashing utilities (no BridgeID formatting). The backend derives any
 * human-readable BridgeID from messageHash off-chain and stores leg-wise tx hashes
 * keyed by messageHash.
 */
library Hashing {
    function _toBytes32(address a) internal pure returns (bytes32) {
        return bytes32(uint256(uint160(a)));
    }

    function payloadHash(bytes memory payload) internal pure returns (bytes32) {
        return keccak256(payload);
    }

    function messageHash(
        uint64 srcChainId,
        address srcAdapter,
        address recipient,
        address asset,
        uint256 amount,
        bytes32 payloadHash_,
        uint64 nonce,
        uint64 dstChainId
    ) internal pure returns (bytes32) {
        return keccak256(
            bytes.concat(
                abi.encodePacked(srcChainId),
                abi.encodePacked(_toBytes32(srcAdapter)),
                abi.encodePacked(_toBytes32(recipient)),
                abi.encodePacked(_toBytes32(asset)),
                abi.encodePacked(amount),
                abi.encodePacked(payloadHash_),
                abi.encodePacked(nonce),
                abi.encodePacked(dstChainId)
            )
        );
    }
}
