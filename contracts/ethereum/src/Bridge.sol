// SPDX-License-Identifier: MIT
pragma solidity ^0.8.30;

import {
    AccessControl
} from "openzeppelin-contracts/contracts/access/AccessControl.sol";

interface IBridge {
    event FundsReceived(uint64 eventId, address indexed sender, uint256 amount);
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

contract Bridge is AccessControl, IBridge {
    error InvalidProcessorKey(bytes key);
    error InvalidValidatorKey(uint256 index, bytes key);
    error DuplicateValidatorKey(uint256 firstIndex, uint256 secondIndex, bytes key);

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

    function _isValidValidatorKey(bytes memory key) internal pure returns (bool) {
        return key.length == 33 && (key[0] == 0x02 || key[0] == 0x03);
    }

    function _requireUniqueKeys(bytes[] memory keys) internal pure {
        for (uint256 i = 0; i < keys.length; i++) {
            for (uint256 j = i + 1; j < keys.length; j++) {
                if (keccak256(keys[i]) == keccak256(keys[j])) {
                    revert DuplicateValidatorKey(i, j, keys[i]);
                }
            }
        }
    }

    constructor(
        address admin,
        bytes memory processor,
        bytes[] memory listeners,
        uint16 neededListeners,
        bytes[] memory approvers,
        uint16 neededApprovers
    ) {
        require(admin != address(0), "Bridge: zero admin");
        if (!_isValidValidatorKey(processor)) {
            revert InvalidProcessorKey(processor);
        }
        require(listeners.length > 0, "Bridge: no listeners");
        for (uint256 i = 0; i < listeners.length; i++) {
            if (!_isValidValidatorKey(listeners[i])) {
                revert InvalidValidatorKey(i, listeners[i]);
            }
        }
        _requireUniqueKeys(listeners);
        require(neededListeners > 0, "Bridge: zero listener quorum");
        require(
            neededListeners <= listeners.length,
            "Bridge: listener quorum too high"
        );
        require(approvers.length > 0, "Bridge: no approvers");
        for (uint256 i = 0; i < approvers.length; i++) {
            if (!_isValidValidatorKey(approvers[i])) {
                revert InvalidValidatorKey(i, approvers[i]);
            }
        }
        _requireUniqueKeys(approvers);
        require(neededApprovers > 0, "Bridge: zero approver quorum");
        require(
            neededApprovers <= approvers.length,
            "Bridge: approver quorum too high"
        );

        _grantRole(DEFAULT_ADMIN_ROLE, admin);

        validatorSet = ValidatorSet({
            processor: processor,
            listeners: listeners,
            neededListeners: neededListeners,
            approvers: approvers,
            neededApprovers: neededApprovers
        });
        nextEventId = 0;
        nextActionId = 0;
    }

    receive() external payable {
        emit FundsReceived(nextEventId, msg.sender, msg.value);
        nextEventId += 1;
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
