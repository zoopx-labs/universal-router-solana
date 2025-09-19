// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

interface IPolicySource {
    enum State {
        Emergency,
        Ok,
        Drain
    }

    function latestOf(address spokeVault)
        external
        view
        returns (uint256 tvlUsd6, uint256 ma7Usd6, uint16 coverageBps, State state, uint64 asOf);
}
