// SPDX-License-Identifier: MIT
pragma solidity ^0.8.30;

import {
    AccessControl
} from "openzeppelin-contracts/contracts/access/AccessControl.sol";

interface IBridgeV1 {
    event FundsReceived(address indexed sender, uint256 amount);
    event AdminPinged(address indexed admin);

    function adminPing() external;
}

contract BridgeV1 is AccessControl, IBridgeV1 {
    struct ValidatorSet {
        // Kolme keys are binary fixed-length data (33-byte compressed pubkey)
        bytes processor;
        bytes[] listeners;
        uint16 neededListeners;
        bytes[] approvers;
        uint16 neededApprovers;
    }

    ValidatorSet internal validatorSet;
    uint64 internal nextEventId;
    uint64 internal nextActionId;

    // `DEFAULT_ADMIN_ROLE` is the only role in v1.
    // It is granted during setup and used for admin-restricted calls.
    constructor(address admin) {
        _grantRole(DEFAULT_ADMIN_ROLE, admin);
    }

    receive() external payable {
        emit FundsReceived(msg.sender, msg.value);
    }

    function adminPing() external onlyRole(DEFAULT_ADMIN_ROLE) {
        emit AdminPinged(msg.sender);
    }
}
