// SPDX-License-Identifier: MIT
pragma solidity ^0.8.30;

import {BridgeActions} from "./BridgeActions.sol";
import {BridgeBase} from "./BridgeBase.sol";

interface IBridge {
    event FundsReceived(
        uint64 indexed eventId,
        address indexed sender,
        uint256 amount
    );
    event Signed(uint64 eventId, address indexed sender, uint64 actionId);
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

contract Bridge is IBridge, BridgeBase, BridgeActions {
    error IncorrectActionId(uint64 expected, uint64 received);
    error InvalidProcessorSignature(address expected, address received);
    error InvalidApproverSignature(address signer);
    error DuplicateApproverSignature(address signer);
    error InsufficientApproverSignatures(uint16 needed, uint256 provided);

    constructor(
        bytes memory processor,
        bytes[] memory listeners,
        uint16 neededListeners,
        bytes[] memory approvers,
        uint16 neededApprovers
    ) {
        _setValidatorSet(
            processor,
            listeners,
            neededListeners,
            approvers,
            neededApprovers
        );
        // Event 0 is reserved for bridge instantiation in kolme.
        nextEventId = 1;
        nextActionId = 0;
    }

    receive() external payable {
        emit FundsReceived(nextEventId, msg.sender, msg.value);
        nextEventId += 1;
    }

    function execute_signed(
        bytes calldata payload,
        bytes calldata processor,
        bytes[] calldata approvers
    ) external {
        (uint64 actionId, bytes memory actionData) = abi.decode(
            payload,
            (uint64, bytes)
        );

        if (actionId != nextActionId) {
            revert IncorrectActionId(nextActionId, actionId);
        }

        bytes32 payloadHash = sha256(payload);
        _verifySignatures(payloadHash, processor, approvers);
        _executeAction(actionData);

        emit Signed(nextEventId, msg.sender, actionId);
        nextEventId += 1;
        nextActionId += 1;
    }

    function _verifySignatures(
        bytes32 payloadHash,
        bytes calldata processor,
        bytes[] calldata approvers
    ) internal view {
        address expectedProcessorSigner = _validatorAddress(
            validatorSet.processor
        );
        address processorSignerRecovered = _recoverSigner(
            payloadHash,
            processor
        );
        if (processorSignerRecovered != expectedProcessorSigner) {
            revert InvalidProcessorSignature(
                expectedProcessorSigner,
                processorSignerRecovered
            );
        }

        uint256 configuredApproverCount = validatorSet.approvers.length;
        address[] memory configuredApproverSigners = new address[](
            configuredApproverCount
        );
        for (uint256 i = 0; i < configuredApproverCount; i++) {
            configuredApproverSigners[i] = _validatorAddress(
                validatorSet.approvers[i]
            );
        }

        uint256 approverCount = approvers.length;
        uint16 neededApprovers = validatorSet.neededApprovers;
        address[] memory seen = new address[](approverCount);
        uint256 uniqueApprovers = 0;
        for (uint256 i = 0; i < approverCount; i++) {
            address signer = _recoverSigner(payloadHash, approvers[i]);

            bool isConfiguredApprover = false;
            for (uint256 j = 0; j < configuredApproverCount; j++) {
                if (configuredApproverSigners[j] == signer) {
                    isConfiguredApprover = true;
                    break;
                }
            }
            if (!isConfiguredApprover) {
                revert InvalidApproverSignature(signer);
            }

            for (uint256 j = 0; j < uniqueApprovers; j++) {
                if (seen[j] == signer) {
                    revert DuplicateApproverSignature(signer);
                }
            }
            seen[uniqueApprovers] = signer;
            uniqueApprovers += 1;
            if (uniqueApprovers == neededApprovers) {
                break;
            }
        }
        if (uniqueApprovers < neededApprovers) {
            revert InsufficientApproverSignatures(
                neededApprovers,
                uniqueApprovers
            );
        }
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
