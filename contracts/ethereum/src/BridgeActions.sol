// SPDX-License-Identifier: MIT
pragma solidity ^0.8.30;

import {BridgeBase} from "./BridgeBase.sol";

abstract contract BridgeActions is BridgeBase {
    uint8 internal constant ACTION_TRANSFER_ETH = 0;
    uint8 internal constant ACTION_TRANSFER_ERC20 = 1;
    uint8 internal constant ACTION_SELF_REPLACE = 2;
    uint8 internal constant ACTION_NEW_SET = 3;

    uint8 internal constant VALIDATOR_LISTENER = 0;
    uint8 internal constant VALIDATOR_PROCESSOR = 1;
    uint8 internal constant VALIDATOR_APPROVER = 2;

    error InvalidActionType(uint8 actionType);
    error InvalidValidatorType(uint8 validatorType);
    error CurrentValidatorNotFound(uint8 validatorType, bytes current);
    error TransferEthFailed(address recipient, uint256 amount);

    function _executeAction(bytes memory actionData) internal {
        (uint8 actionType, bytes memory data) = abi.decode(
            actionData,
            (uint8, bytes)
        );
        if (actionType == ACTION_TRANSFER_ETH) {
            _executeTransferEth(data);
            return;
        }
        if (actionType == ACTION_SELF_REPLACE) {
            _executeSelfReplace(data);
            return;
        }
        if (actionType == ACTION_NEW_SET) {
            _executeNewSet(data);
            return;
        }

        revert InvalidActionType(actionType);
    }

    function _executeTransferEth(bytes memory data) internal {
        (address recipient, uint256 amount) = abi.decode(
            data,
            (address, uint256)
        );
        (bool success, ) = recipient.call{value: amount}("");
        if (!success) {
            revert TransferEthFailed(recipient, amount);
        }
    }

    function _executeSelfReplace(bytes memory data) internal {
        (
            uint8 validatorType,
            bytes memory current,
            bytes memory replacement
        ) = abi.decode(data, (uint8, bytes, bytes));
        if (!_isValidValidatorKey(replacement)) {
            revert InvalidValidatorKey(0, replacement);
        }
        _validatorAddress(replacement);

        if (validatorType == VALIDATOR_PROCESSOR) {
            if (keccak256(current) != keccak256(validatorSet.processor)) {
                revert CurrentValidatorNotFound(validatorType, current);
            }
            validatorSet.processor = replacement;
            return;
        }
        if (validatorType == VALIDATOR_LISTENER) {
            _replaceValidatorInArray(
                validatorType,
                validatorSet.listeners,
                current,
                replacement
            );
            return;
        }
        if (validatorType == VALIDATOR_APPROVER) {
            _replaceValidatorInArray(
                validatorType,
                validatorSet.approvers,
                current,
                replacement
            );
            return;
        }

        revert InvalidValidatorType(validatorType);
    }

    function _replaceValidatorInArray(
        uint8 validatorType,
        bytes[] storage keys,
        bytes memory current,
        bytes memory replacement
    ) internal {
        uint256 currentIndex = _findValidatorIndex(keys, current);
        if (currentIndex == type(uint256).max) {
            revert CurrentValidatorNotFound(validatorType, current);
        }
        uint256 replacementIndex = _findValidatorIndex(keys, replacement);
        if (
            replacementIndex != type(uint256).max &&
            replacementIndex != currentIndex
        ) {
            revert DuplicateValidatorKey(
                currentIndex,
                replacementIndex,
                replacement
            );
        }
        keys[currentIndex] = replacement;
    }

    function _findValidatorIndex(
        bytes[] storage keys,
        bytes memory target
    ) internal view returns (uint256) {
        for (uint256 i = 0; i < keys.length; i++) {
            if (keccak256(keys[i]) == keccak256(target)) {
                return i;
            }
        }
        return type(uint256).max;
    }

    function _executeNewSet(bytes memory data) internal {
        (
            bytes memory processor,
            bytes[] memory listeners,
            uint16 neededListeners,
            bytes[] memory approvers,
            uint16 neededApprovers
        ) = abi.decode(data, (bytes, bytes[], uint16, bytes[], uint16));
        _setValidatorSet(
            processor,
            listeners,
            neededListeners,
            approvers,
            neededApprovers
        );
    }
}
