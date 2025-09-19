// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;

contract Create2Factory {
    address public owner;

    constructor() {
        owner = msg.sender;
    }

    modifier onlyOwner() {
        require(msg.sender == owner, "not owner");
        _;
    }
    event Deployed(address addr, bytes32 salt);

    function deploy(bytes32 salt, bytes calldata creationCode) external payable returns (address addr) {
        // creationCode = bytecode (+ encoded constructor args)
        // Copy calldata bytes to memory to satisfy solc/yul rules.
        bytes memory code = creationCode;
        assembly {
            let data := add(code, 0x20)
            let size := mload(code)
            addr := create2(callvalue(), data, size, salt)
            if iszero(extcodesize(addr)) { revert(0, 0) }
        }
        emit Deployed(addr, salt);
    }

    // Helper for off-chain precompute checks
    function compute(bytes32 salt, bytes32 creationCodeHash) external view returns (address) {
        return
            address(uint160(uint256(keccak256(abi.encodePacked(bytes1(0xff), address(this), salt, creationCodeHash)))));
    }

    // Withdraw any ETH accidentally sent (silences Slither 'locks ether').
    function withdraw(address payable to) external onlyOwner {
        require(to != address(0), "zero to");
        to.transfer(address(this).balance);
    }
}
