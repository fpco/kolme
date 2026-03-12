// SPDX-License-Identifier: MIT
pragma solidity ^0.8.30;

abstract contract BridgeBase {
    error EmptyListeners();
    error EmptyApprovers();
    error InvalidListenerQuorum(uint16 needed, uint256 total);
    error InvalidApproverQuorum(uint16 needed, uint256 total);
    error InvalidProcessorKey(bytes key);
    error InvalidValidatorKey(uint256 index, bytes key);
    error DuplicateValidatorKey(
        uint256 firstIndex,
        uint256 secondIndex,
        bytes key
    );
    error InvalidCurvePoint(bytes key);
    error InvalidSignatureLength(uint256 length);
    error InvalidSignatureV(uint8 v);
    error ModExpFailed();
    error InvalidApproverSignature(address signer);
    error DuplicateApproverSignature(address signer);
    error InsufficientApproverSignatures(uint16 needed, uint256 provided);

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

    uint256 internal constant SECP256K1_P =
        0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFC2F;
    uint256 internal constant SECP256K1_SQRT_EXP =
        0x3FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFBFFFFF0C;

    function _isValidValidatorKey(
        bytes memory key
    ) internal pure returns (bool) {
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

    function _setValidatorSet(
        bytes memory processor,
        bytes[] memory listeners,
        uint16 neededListeners,
        bytes[] memory approvers,
        uint16 neededApprovers
    ) internal {
        if (!_isValidValidatorKey(processor)) {
            revert InvalidProcessorKey(processor);
        }
        _validatorAddress(processor);
        if (listeners.length == 0) {
            revert EmptyListeners();
        }
        for (uint256 i = 0; i < listeners.length; i++) {
            if (!_isValidValidatorKey(listeners[i])) {
                revert InvalidValidatorKey(i, listeners[i]);
            }
            _validatorAddress(listeners[i]);
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
            _validatorAddress(approvers[i]);
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
    }

    // Kolme validators don't really have Ethereum addresses (as they are identified
    // by secp256k1 public keys), but we can derive (cryptographically) EVM addresses
    // from these public keys to use them for verifying their signatures by comparing
    // to _recoverSigner() output, which is EVM address derived from
    // payload hash + signature.
    function _validatorAddress(
        bytes memory key
    ) internal view returns (address) {
        if (!_isValidValidatorKey(key)) {
            revert InvalidCurvePoint(key);
        }
        uint256 x = 0;
        for (uint256 i = 1; i < 33; i++) {
            x = (x << 8) | uint8(key[i]);
        }
        if (x >= SECP256K1_P) {
            revert InvalidCurvePoint(key);
        }

        uint256 ySquared = addmod(
            mulmod(mulmod(x, x, SECP256K1_P), x, SECP256K1_P),
            7,
            SECP256K1_P
        );
        uint256 y = _modExp(ySquared, SECP256K1_SQRT_EXP, SECP256K1_P);
        if (mulmod(y, y, SECP256K1_P) != ySquared) {
            revert InvalidCurvePoint(key);
        }

        bool expectedOdd = key[0] == 0x03;
        bool yOdd = (y & 1) == 1;
        if (yOdd != expectedOdd) {
            y = SECP256K1_P - y;
        }

        return
            address(
                uint160(
                    uint256(keccak256(abi.encodePacked(bytes32(x), bytes32(y))))
                )
            );
    }

    function _modExp(
        uint256 base,
        uint256 exponent,
        uint256 modulus
    ) internal view returns (uint256 result) {
        bytes memory input = abi.encode(
            uint256(32),
            uint256(32),
            uint256(32),
            base,
            exponent,
            modulus
        );
        bytes memory output = new bytes(32);
        bool success;
        assembly {
            success := staticcall(
                gas(),
                0x05,
                add(input, 0x20),
                mload(input),
                add(output, 0x20),
                0x20
            )
        }
        if (!success) {
            revert ModExpFailed();
        }
        return abi.decode(output, (uint256));
    }

    // Returns signer's address recovered from a payload hash and ECDSA signature.
    // Used for verifying processor/approvers signatures.
    function _recoverSigner(
        bytes32 payloadHash,
        bytes memory signature
    ) internal pure returns (address) {
        if (signature.length != 65) {
            revert InvalidSignatureLength(signature.length);
        }
        bytes32 r;
        bytes32 s;
        uint8 v;
        assembly {
            r := mload(add(signature, 0x20))
            s := mload(add(signature, 0x40))
            v := byte(0, mload(add(signature, 0x60)))
        }
        if (v != 27 && v != 28) {
            revert InvalidSignatureV(v);
        }
        return ecrecover(payloadHash, v, r, s);
    }

    function _containsSigner(
        bytes[] storage keys,
        address signer
    ) internal view returns (bool) {
        for (uint256 i = 0; i < keys.length; i++) {
            if (_validatorAddress(keys[i]) == signer) {
                return true;
            }
        }
        return false;
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
}
