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
    error InvalidNewSetApprovalSignature(address signer);
    error DuplicateNewSetApprovalSignature(address signer);
    error InsufficientNewSetGroupSignatures(uint8 needed, uint8 provided);

    function _expectedSignersForAction(
        bytes memory actionData
    )
        internal
        view
        returns (
            bytes memory expectedProcessor,
            bytes[] memory expectedApprovers,
            uint16 neededApprovers
        )
    {
        expectedProcessor = validatorSet.processor;
        expectedApprovers = _copyValidatorKeys(validatorSet.approvers);
        neededApprovers = validatorSet.neededApprovers;

        (uint8 actionType, bytes memory data) = abi.decode(
            actionData,
            (uint8, bytes)
        );
        if (actionType == ACTION_NEW_SET) {
            (
                bytes memory newProcessor,
                bytes[] memory newListeners,
                uint16 newNeededListeners,
                bytes[] memory newApprovers,
                uint16 newNeededApprovers,
                bytes memory rendered,
                bytes[] memory approvals
            ) = abi.decode(
                    data,
                    (bytes, bytes[], uint16, bytes[], uint16, bytes, bytes[])
                );
            _verifyNewSetApprovals(rendered, approvals);
            // Silence unused local warnings while still validating tuple shape.
            newListeners;
            newNeededListeners;
            return (newProcessor, newApprovers, newNeededApprovers);
        }
        if (actionType != ACTION_SELF_REPLACE) {
            return (expectedProcessor, expectedApprovers, neededApprovers);
        }

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
            expectedProcessor = replacement;
            return (expectedProcessor, expectedApprovers, neededApprovers);
        }
        if (validatorType == VALIDATOR_LISTENER) {
            uint256 currentIndex = _findValidatorIndex(
                validatorSet.listeners,
                current
            );
            if (currentIndex == type(uint256).max) {
                revert CurrentValidatorNotFound(validatorType, current);
            }
            uint256 replacementIndex = _findValidatorIndex(
                validatorSet.listeners,
                replacement
            );
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
            return (expectedProcessor, expectedApprovers, neededApprovers);
        }
        if (validatorType == VALIDATOR_APPROVER) {
            uint256 currentIndex = _findValidatorIndex(
                validatorSet.approvers,
                current
            );
            if (currentIndex == type(uint256).max) {
                revert CurrentValidatorNotFound(validatorType, current);
            }
            uint256 replacementIndex = _findValidatorIndex(
                validatorSet.approvers,
                replacement
            );
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
            expectedApprovers[currentIndex] = replacement;
            return (expectedProcessor, expectedApprovers, neededApprovers);
        }

        revert InvalidValidatorType(validatorType);
    }

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

    function _copyValidatorKeys(
        bytes[] storage keys
    ) internal view returns (bytes[] memory copied) {
        copied = new bytes[](keys.length);
        for (uint256 i = 0; i < keys.length; i++) {
            copied[i] = keys[i];
        }
    }

    function _executeNewSet(bytes memory data) internal {
        (
            bytes memory processor,
            bytes[] memory listeners,
            uint16 neededListeners,
            bytes[] memory approvers,
            uint16 neededApprovers,
            bytes memory rendered,
            bytes[] memory approvals
        ) = abi.decode(
                data,
                (bytes, bytes[], uint16, bytes[], uint16, bytes, bytes[])
            );
        rendered;
        approvals;
        _setValidatorSet(
            processor,
            listeners,
            neededListeners,
            approvers,
            neededApprovers
        );
    }

    function _verifyNewSetApprovals(
        bytes memory rendered,
        bytes[] memory approvals
    ) internal view {
        bytes32 renderedHash = sha256(rendered);

        bool hasProcessor = false;
        uint256 listeners = 0;
        uint256 approvers = 0;
        uint256 approvalsCount = approvals.length;
        address[] memory seen = new address[](approvalsCount);
        uint256 uniqueSigners = 0;

        address processorSigner = _validatorAddress(validatorSet.processor);
        for (uint256 i = 0; i < approvalsCount; i++) {
            address signer = _recoverSigner(renderedHash, approvals[i]);

            for (uint256 j = 0; j < uniqueSigners; j++) {
                if (seen[j] == signer) {
                    revert DuplicateNewSetApprovalSignature(signer);
                }
            }
            seen[uniqueSigners] = signer;
            uniqueSigners += 1;

            bool inCurrentSet = false;
            if (signer == processorSigner) {
                hasProcessor = true;
                inCurrentSet = true;
            }
            if (_containsSigner(validatorSet.listeners, signer)) {
                listeners += 1;
                inCurrentSet = true;
            }
            if (_containsSigner(validatorSet.approvers, signer)) {
                approvers += 1;
                inCurrentSet = true;
            }
            if (!inCurrentSet) {
                revert InvalidNewSetApprovalSignature(signer);
            }
        }

        uint8 groupApprovals = (hasProcessor ? 1 : 0) +
            (listeners >= validatorSet.neededListeners ? 1 : 0) +
            (approvers >= validatorSet.neededApprovers ? 1 : 0);

        if (groupApprovals < 2) {
            revert InsufficientNewSetGroupSignatures(2, groupApprovals);
        }
    }
}
