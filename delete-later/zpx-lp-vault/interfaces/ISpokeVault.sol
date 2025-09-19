// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

interface ISpokeVault {
    function asset() external view returns (address);
    function totalAssets() external view returns (uint256);
    function maxBorrow() external view returns (uint256);
    function borrow(uint256 amount, address to) external returns (uint256);
    function repay(uint256 amount) external returns (uint256);
    function idleLiquidity() external view returns (uint256);
    function utilizationBps() external view returns (uint16);
}
