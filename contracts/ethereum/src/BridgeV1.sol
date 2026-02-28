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
