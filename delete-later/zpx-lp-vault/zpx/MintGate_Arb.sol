// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {AccessControl} from "@openzeppelin/contracts/access/AccessControl.sol";
import {ZPXArb} from "./ZPXArb.sol";

/**
 * @title MintGate_Arb
 * @dev Gate contract that mints ZPXArb on verified cross-chain messages from Base.
 */
contract MintGate_Arb is Ownable {
    address public immutable zpx;
    mapping(bytes32 => bool) public used;
    address public allowedSrc;
    uint64 public allowedSrcChainId;

    event EndpointUpdated(uint64 indexed srcChainId, address indexed srcContract);
    event MintOnMessage(address indexed recipient, uint256 amount, bytes32 purpose, uint256 nonce);

    constructor(address zpx_) Ownable(msg.sender) {
        require(zpx_ != address(0), "ZPX=0");
        zpx = zpx_;
    }

    function setEndpoint(uint64 srcChainId, address srcContract) external onlyOwner {
        require(srcContract != address(0), "SRC=0");
        allowedSrcChainId = srcChainId;
        allowedSrc = srcContract;
        emit EndpointUpdated(srcChainId, srcContract);
    }

    function consumeAndMint(
        uint64 srcChainId,
        address srcContract,
        uint256 nonce,
        address recipient,
        uint256 amount,
        bytes32 purpose
    ) external {
        require(srcChainId == allowedSrcChainId, "bad chain");
        require(srcContract == allowedSrc, "bad src");
        bytes32 key = keccak256(abi.encode(srcChainId, srcContract, nonce, recipient, amount, purpose));
        require(!used[key], "replay");
        used[key] = true;
        emit MintOnMessage(recipient, amount, purpose, nonce);
        ZPXArb(zpx).mint(recipient, amount);
    }
}
