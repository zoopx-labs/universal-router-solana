// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

interface IPpsSource {
    function latestPps6() external view returns (uint256 pps6, uint64 asOf);
}
