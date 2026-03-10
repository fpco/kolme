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
        (
            bytes memory expectedProcessor,
            bytes[] memory expectedApprovers,
            uint16 neededApprovers
        ) = _expectedSignersForAction(actionData);
        _verifySignatures(
            payloadHash,
            processor,
            approvers,
            expectedProcessor,
            expectedApprovers,
            neededApprovers
        );
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

        address[] memory configuredApproverSigners = _validatorAddresses(
            expectedApprovers
        );
        _verifyApproverSignatures(
            payloadHash,
            approvers,
            configuredApproverSigners,
            neededApprovers
        );
    }

    function _verifyApproverSignatures(
        bytes32 payloadHash,
        bytes[] calldata approvers,
        address[] memory configuredApproverSigners,
        uint16 neededApprovers
    ) internal pure {
        uint256 configuredApproverCount = configuredApproverSigners.length;
        uint256 approverCount = approvers.length;
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

    function _validatorAddresses(
        bytes[] memory validatorKeys
    ) internal view returns (address[] memory result) {
        uint256 count = validatorKeys.length;
        result = new address[](count);
        for (uint256 i = 0; i < count; i++) {
            result[i] = _validatorAddress(validatorKeys[i]);
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
