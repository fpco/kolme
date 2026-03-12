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
    // forge-lint: disable-next-line(mixed-case-function)
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

    // forge-lint: disable-next-line(mixed-case-function)
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
        (
            bytes memory expectedProcessor,
            bytes[] memory expectedApprovers,
            uint16 neededApprovers
        ) = _expectedSignersForAction(actionData);

        // First - verify signatures from the outer layer - signatures for action execution
        _verifySignatures(
            payloadHash,
            processor,
            approvers,
            expectedProcessor,
            expectedApprovers,
            neededApprovers
        );

        // Second - verify signatures in the inner layer - the action proposal
        // and validate action payload
        _validateActionForExecution(actionData);

        _executeAction(actionData);

        emit Signed(nextEventId, msg.sender, actionId);
        nextEventId += 1;
        nextActionId += 1;
    }

    function _verifySignatures(
        bytes32 payloadHash,
        bytes calldata processor,
        bytes[] calldata approvers,
        bytes memory expectedProcessor,
        bytes[] memory expectedApprovers,
        uint16 neededApprovers
    ) internal view {
        address expectedProcessorSigner = _validatorAddress(expectedProcessor);
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

        uint256 approverCount = expectedApprovers.length;
        address[] memory configuredApproverSigners = new address[](
            approverCount
        );
        for (uint256 i = 0; i < approverCount; i++) {
            configuredApproverSigners[i] = _validatorAddress(
                expectedApprovers[i]
            );
        }
        _verifyApproverSignatures(
            payloadHash,
            approvers,
            configuredApproverSigners,
            neededApprovers
        );
    }

    // forge-lint: disable-next-line(mixed-case-function)
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
