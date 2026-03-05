// SPDX-License-Identifier: MIT
pragma solidity ^0.8.30;

interface IBridge {
    event FundsReceived(uint64 indexed eventId, address indexed sender, uint256 amount);
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

contract Bridge is IBridge {
    error EmptyListeners();
    error EmptyApprovers();
    error InvalidListenerQuorum(uint16 needed, uint256 total);
    error InvalidApproverQuorum(uint16 needed, uint256 total);
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
        bytes memory processor,
        bytes[] memory listeners,
        uint16 neededListeners,
        bytes[] memory approvers,
        uint16 neededApprovers
    ) {
        if (!_isValidValidatorKey(processor)) {
            revert InvalidProcessorKey(processor);
        }
        if (listeners.length == 0) {
            revert EmptyListeners();
        }
        for (uint256 i = 0; i < listeners.length; i++) {
            if (!_isValidValidatorKey(listeners[i])) {
                revert InvalidValidatorKey(i, listeners[i]);
            }
        }
        _requireUniqueKeys(listeners);
        if (neededListeners == 0 || neededListeners > listeners.length) {
            revert InvalidListenerQuorum(neededListeners, listeners.length);
        }
        if (approvers.length == 0) {
            revert EmptyApprovers();
        }
        for (uint256 i = 0; i < approvers.length; i++) {
            if (!_isValidValidatorKey(approvers[i])) {
                revert InvalidValidatorKey(i, approvers[i]);
            }
        }
        _requireUniqueKeys(approvers);
        if (neededApprovers == 0 || neededApprovers > approvers.length) {
            revert InvalidApproverQuorum(neededApprovers, approvers.length);
        }

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
