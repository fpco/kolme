// SPDX-License-Identifier: MIT
pragma solidity ^0.8.30;

import {BridgeActions} from "./BridgeActions.sol";
import {BridgeBase} from "./BridgeBase.sol";
import {
    ReentrancyGuard
} from "../lib/openzeppelin-contracts/contracts/utils/ReentrancyGuard.sol";
import {
    IERC20
} from "../lib/openzeppelin-contracts/contracts/token/ERC20/IERC20.sol";

interface IBridge {
    event FundsReceived(
        uint64 indexed eventId,
        address indexed sender,
        address[] tokens,
        uint256[] amounts,
        bytes[] keys
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

contract Bridge is IBridge, BridgeBase, BridgeActions, ReentrancyGuard {
    error IncorrectActionId(uint64 expected, uint64 received);
    error InvalidProcessorSignature(address expected, address received);
    error InsufficientAllowance(
        address token,
        address owner,
        uint256 allowed,
        uint256 required
    );
    error ERC20TransferFailed(address token, address from, uint256 amount);
    error RegularFundsLengthMismatch(uint256 tokens, uint256 amounts);
    error EthValueMismatch(uint256 expected, uint256 received);
    error InvalidRegularKey(uint256 index, bytes key);
    error InvalidRegularKeySignature(
        uint256 index,
        address expected,
        address received
    );

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

    function regular(
        address[] calldata tokens,
        uint256[] calldata amounts,
        bytes[] calldata keys
    ) external payable nonReentrant {
        uint256 transferCount = tokens.length;
        if (transferCount != amounts.length) {
            revert RegularFundsLengthMismatch(transferCount, amounts.length);
        }

        bytes32 regularHash = sha256(abi.encodePacked(msg.sender));
        uint256 regularKeyCount = keys.length;
        bytes[] memory regularKeys = new bytes[](regularKeyCount);
        for (uint256 i = 0; i < regularKeyCount; i++) {
            (bytes memory key, bytes memory signature) = abi.decode(
                keys[i],
                (bytes, bytes)
            );
            if (!_isValidSecp256k1Pubkey(key)) {
                revert InvalidRegularKey(i, key);
            }
            address expected = _validatorAddress(key);
            address received = _recoverSigner(regularHash, signature);
            if (received != expected) {
                revert InvalidRegularKeySignature(i, expected, received);
            }
            regularKeys[i] = key;
        }

        uint256 ethExpectedValue = 0;
        for (uint256 i = 0; i < transferCount; i++) {
            address token = tokens[i];
            uint256 amount = amounts[i];
            // Native ETH leg, value is checked after iterating all entries.
            if (token == address(0)) {
                ethExpectedValue += amount;
                continue;
            }

            IERC20 erc20 = IERC20(token);
            uint256 allowed = erc20.allowance(msg.sender, address(this));
            if (allowed < amount) {
                revert InsufficientAllowance(token, msg.sender, allowed, amount);
            }
            bool success = erc20.transferFrom(msg.sender, address(this), amount);
            if (!success) {
                revert ERC20TransferFailed(token, msg.sender, amount);
            }
        }

        if (msg.value != ethExpectedValue) {
            revert EthValueMismatch(ethExpectedValue, msg.value);
        }

        emit FundsReceived(nextEventId, msg.sender, tokens, amounts, regularKeys);
        nextEventId += 1;
    }

    // forge-lint: disable-next-line(mixed-case-function)
    function execute_signed(
        bytes calldata payload,
        bytes calldata processor,
        bytes[] calldata approvers
    ) external nonReentrant {
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
