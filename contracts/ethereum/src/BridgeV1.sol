// SPDX-License-Identifier: MIT
pragma solidity ^0.8.30;

import {
    AccessControl
} from "openzeppelin-contracts/contracts/access/AccessControl.sol";

interface IBridgeV1 {
    event FundsReceived(address indexed sender, uint256 amount);
    event AdminPinged(address indexed admin);

    function adminPing() external;
    function get_config()
        external
        view
        returns (
            bytes memory processor,
            bytes[] memory listeners,
            uint16 neededListeners,
            bytes[] memory approvers,
            uint16 neededApprovers,
            uint64 configNextEventId,
            uint64 configNextActionId
        );
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

    constructor(
        address admin,
        bytes memory processor,
        bytes[] memory listeners,
        uint16 neededListeners,
        bytes[] memory approvers,
        uint16 neededApprovers,
        uint64 initialNextEventId,
        uint64 initialNextActionId
    ) {
        require(admin != address(0), "BridgeV1: zero admin");
        require(processor.length == 33, "BridgeV1: invalid processor key");
        require(listeners.length > 0, "BridgeV1: no listeners");
        for (uint256 i = 0; i < listeners.length; i++) {
            require(
                listeners[i].length == 33,
                "BridgeV1: invalid listener key length"
            );
        }
        require(neededListeners > 0, "BridgeV1: zero listener quorum");
        require(
            neededListeners <= listeners.length,
            "BridgeV1: listener quorum too high"
        );
        require(approvers.length > 0, "BridgeV1: no approvers");
        for (uint256 i = 0; i < approvers.length; i++) {
            require(
                approvers[i].length == 33,
                "BridgeV1: invalid approver key length"
            );
        }
        require(neededApprovers > 0, "BridgeV1: zero approver quorum");
        require(
            neededApprovers <= approvers.length,
            "BridgeV1: approver quorum too high"
        );

        _grantRole(DEFAULT_ADMIN_ROLE, admin);

        validatorSet = ValidatorSet({
            processor: processor,
            listeners: listeners,
            neededListeners: neededListeners,
            approvers: approvers,
            neededApprovers: neededApprovers
        });
        nextEventId = initialNextEventId;
        nextActionId = initialNextActionId;
    }

    receive() external payable {
        emit FundsReceived(msg.sender, msg.value);
    }

    function adminPing() external onlyRole(DEFAULT_ADMIN_ROLE) {
        emit AdminPinged(msg.sender);
    }

    function get_config()
        external
        view
        returns (
            bytes memory processor,
            bytes[] memory listeners,
            uint16 neededListeners,
            bytes[] memory approvers,
            uint16 neededApprovers,
            uint64 configNextEventId,
            uint64 configNextActionId
        )
    {
        ValidatorSet storage set = validatorSet;
        return (
            set.processor,
            set.listeners,
            set.neededListeners,
            set.approvers,
            set.neededApprovers,
            nextEventId,
            nextActionId
        );
    }
}
