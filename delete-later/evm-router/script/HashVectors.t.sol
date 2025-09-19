// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Script.sol";
import {Hashing} from "../lib/Hashing.sol";

contract HashVectors is Script {
    struct MsgHashCase {
        uint64 src_chain_id;
        uint64 dst_chain_id;
        uint64 nonce;
        address src_adapter;
        address recipient;
        address asset;
        bytes amount_be_hex; // 32 bytes BE
        bytes payload_hex;
        address initiator;
        bytes32 expected_message_hash_hex;
        bytes32 expected_global_route_id_hex;
    }

    function addr(bytes20 a) internal pure returns (address) {
        return address(uint160(uint256(bytes32(a))));
    }

    function run() external {
        // Define the same three cases as Rust generator
        MsgHashCase[] memory cases = new MsgHashCase[](3);
        // Case 1
        cases[0] = MsgHashCase({
            src_chain_id: 42161,
            dst_chain_id: 8453,
            nonce: 42,
            src_adapter: 0x1111111111111111111111111111111111111111,
            recipient: 0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa,
            asset: 0x2222222222222222222222222222222222222222,
            amount_be_hex: hex"000000000000000000000000000000000000000000000000000000000001e240",
            payload_hex: hex"deadbeef",
            initiator: 0x3333333333333333333333333333333333333333,
            expected_message_hash_hex: bytes32(0),
            expected_global_route_id_hex: bytes32(0)
        });
        // Case 2
        cases[1] = MsgHashCase({
            src_chain_id: 1,
            dst_chain_id: 2,
            nonce: 1,
            src_adapter: 0x0000000000000000000000000000000000000001,
            recipient: 0x0000000000000000000000000000000000000002,
            asset: 0x0000000000000000000000000000000000000003,
            amount_be_hex: hex"000000000000000000000000000000000000000000000000ffffffffffffffff",
            payload_hex: hex"",
            initiator: 0x0000000000000000000000000000000000000004,
            expected_message_hash_hex: bytes32(0),
            expected_global_route_id_hex: bytes32(0)
        });
        // Case 3
        cases[2] = MsgHashCase({
            src_chain_id: 10,
            dst_chain_id: 56,
            nonce: 9999,
            src_adapter: 0x1234567890abcdef1234567890abcdef12345678,
            recipient: 0xabcdefabcdefabcdefabcdefabcdefabcdefabcd,
            asset: 0x9999999999999999999999999999999999999999,
            amount_be_hex: hex"0000000000000000000000000000000000000000000000000de0b6b3a7640000",
            payload_hex: hex"0102030405",
            initiator: 0x7777777777777777777777777777777777777777,
            expected_message_hash_hex: bytes32(0),
            expected_global_route_id_hex: bytes32(0)
        });

        // Compute expected fields and output JSON
        string memory json = string(abi.encodePacked('{"message_hashes":['));
        for (uint i = 0; i < cases.length; i++) {
            MsgHashCase memory c = cases[i];
            bytes32 pHash = Hashing.payloadHash(c.payload_hex);
            uint256 amount = uint256(bytes32(c.amount_be_hex));
            bytes32 msgHash = Hashing.messageHash(
                c.src_chain_id,
                c.src_adapter,
                c.recipient,
                c.asset,
                amount,
                pHash,
                c.nonce,
                c.dst_chain_id
            );
            bytes32 global = keccak256(
                bytes.concat(
                    abi.encodePacked(c.src_chain_id),
                    abi.encodePacked(c.dst_chain_id),
                    abi.encodePacked(Hashing._toBytes32(c.initiator)),
                    abi.encodePacked(msgHash),
                    abi.encodePacked(c.nonce)
                )
            );
            json = string(
                abi.encodePacked(
                    json,
                    '{',
                    '"src_chain_id":', vm.toString(c.src_chain_id), ',',
                    '"dst_chain_id":', vm.toString(c.dst_chain_id), ',',
                    '"nonce":', vm.toString(c.nonce), ',',
                    '"src_adapter":"0x', vm.toStringHex(address(c.src_adapter)), '",',
                    '"recipient":"0x', vm.toStringHex(address(c.recipient)), '",',
                    '"asset":"0x', vm.toStringHex(address(c.asset)), '",',
                    '"amount_be_hex":"0x', vm.toStringHex(bytes32(c.amount_be_hex)), '",',
                    '"payload_hex":"0x', vm.toStringHex(c.payload_hex), '",',
                    '"initiator":"0x', vm.toStringHex(address(c.initiator)), '",',
                    '"expected_message_hash_hex":"0x', vm.toStringHex(msgHash), '",',
                    '"expected_global_route_id_hex":"0x', vm.toStringHex(global), '"',
                    '}'
                )
            );
            if (i + 1 < cases.length) {
                json = string(abi.encodePacked(json, ','));
            }
        }
        json = string(abi.encodePacked(json, ']}'));
        console2.log(json);
    }
}
