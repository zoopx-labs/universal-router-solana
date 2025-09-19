// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

contract MockDIAFeed {
    int256 public price;
    uint256 public ts;

    constructor(int256 p, uint256 _ts) {
        price = p;
        ts = _ts == 0 ? block.timestamp : _ts;
    }

    function set(int256 p, uint256 _ts) external {
        price = p;
        ts = _ts;
    }

    function latestValue() external view returns (int256, uint256) {
        return (price, ts);
    }
}
