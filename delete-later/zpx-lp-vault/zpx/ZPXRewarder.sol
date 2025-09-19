// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {AccessControl} from "@openzeppelin/contracts/access/AccessControl.sol";

/**
 * @title ZPXRewarder
 * @dev Rewarder that distributes prefunded ZPXArb rewards to USDzy stakers via epochs/top-ups.
 */
contract ZPXRewarder is AccessControl {
    using SafeERC20 for IERC20;

    bytes32 public constant TOPUP_ROLE = keccak256("TOPUP_ROLE");

    IERC20 public immutable stakeToken; // USDzy
    IERC20 public immutable rewardToken; // ZPXArb

    uint256 public totalStaked;
    // Accumulator scaled by 1e18. Not constant; updated over time via funding logic.
    // slither-disable-next-line const-state
    uint256 public accPerShare = 0; // 1e18
    uint256 public rewardsAccrued;

    uint64 public startTime;
    uint64 public endTime;
    uint256 public rewardRatePerSec;

    uint256 public lastUpdate;

    struct UserInfo {
        uint256 amount;
        uint256 rewardDebt;
    }

    mapping(address => UserInfo) public users;

    event Funded(uint256 rate, uint64 start, uint64 end);
    event TopUpReceived(uint256 amount);
    event Deposit(address indexed user, uint256 amount);
    event Withdraw(address indexed user, uint256 amount);
    event Claim(address indexed user, uint256 amount);

    constructor(address stake_, address reward_, address admin) {
        stakeToken = IERC20(stake_);
        rewardToken = IERC20(reward_);
        _grantRole(DEFAULT_ADMIN_ROLE, admin);
    }

    // internal update
    function _update() internal {
        // Using block.timestamp for reward accrual is acceptable here:
        // rewards are linear over time and miner manipulation has negligible impact.
        // slither-disable-next-line timestamp
        if (lastUpdate < 1) return;
        // slither-disable-next-line timestamp
        uint256 to = block.timestamp < endTime ? block.timestamp : endTime;
        if (to <= lastUpdate) return;
        uint256 elapsed = to - lastUpdate;
        uint256 newRewards = elapsed * rewardRatePerSec;
        rewardsAccrued += newRewards;
        if (totalStaked > 0) {
            // scale accPerShare by 1e18
            accPerShare += (newRewards * 1e18) / totalStaked;
        }
        lastUpdate = to;
    }

    function deposit(uint256 amt) external {
        _update();
        UserInfo storage u = users[msg.sender];
        if (u.amount > 0) {
            uint256 pending = (u.amount * accPerShare) / 1e18 - u.rewardDebt;
            if (pending > 0) {
                rewardToken.safeTransfer(msg.sender, pending);
                emit Claim(msg.sender, pending);
            }
        }
        stakeToken.safeTransferFrom(msg.sender, address(this), amt);
        u.amount += amt;
        totalStaked += amt;
        u.rewardDebt = (u.amount * accPerShare) / 1e18;
        emit Deposit(msg.sender, amt);
    }

    function withdraw(uint256 amt) external {
        _update();
        UserInfo storage u = users[msg.sender];
        require(u.amount >= amt, "insuff");
        uint256 pending = (u.amount * accPerShare) / 1e18 - u.rewardDebt;
        if (pending > 0) {
            rewardToken.safeTransfer(msg.sender, pending);
            emit Claim(msg.sender, pending);
        }
        u.amount -= amt;
        totalStaked -= amt;
        stakeToken.safeTransfer(msg.sender, amt);
        u.rewardDebt = (u.amount * accPerShare) / 1e18;
        emit Withdraw(msg.sender, amt);
    }

    function claim() external {
        _update();
        UserInfo storage u = users[msg.sender];
        uint256 pending = (u.amount * accPerShare) / 1e18 - u.rewardDebt;
        require(pending > 0, "no reward");
        rewardToken.safeTransfer(msg.sender, pending);
        u.rewardDebt = (u.amount * accPerShare) / 1e18;
        emit Claim(msg.sender, pending);
    }

    function emergencyWithdraw() external {
        UserInfo storage u = users[msg.sender];
        uint256 amt = u.amount;
        require(amt > 0, "zero");
        u.amount = 0;
        u.rewardDebt = 0;
        totalStaked -= amt;
        stakeToken.safeTransfer(msg.sender, amt);
        emit Withdraw(msg.sender, amt);
    }

    // only TOPUP_ROLE (MintGate) can call
    function notifyTopUp(uint256 amount, uint64 durationSecs) external onlyRole(TOPUP_ROLE) {
        require(durationSecs > 0, "duration0");
        // reward tokens are expected to be already transferred to this contract
        // merge logic
        _update();
        uint64 nowT = uint64(block.timestamp);
        if (block.timestamp >= endTime) {
            rewardRatePerSec = amount / durationSecs;
            startTime = nowT;
            endTime = nowT + durationSecs;
            lastUpdate = nowT;
        } else {
            uint256 remaining = (endTime > block.timestamp) ? (endTime - block.timestamp) : 0;
            uint256 leftover = remaining * rewardRatePerSec;
            uint256 newTotal = leftover + amount;
            uint256 newDuration = remaining + durationSecs;
            rewardRatePerSec = newTotal / newDuration;
            endTime = uint64(block.timestamp + newDuration);
            // continue accrual from now
            lastUpdate = block.timestamp;
        }
        emit TopUpReceived(amount);
        emit Funded(rewardRatePerSec, startTime, endTime);
    }
}
