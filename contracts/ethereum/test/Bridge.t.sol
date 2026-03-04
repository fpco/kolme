// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.30;

import {Test} from "forge-std/Test.sol";
import {Bridge} from "../src/Bridge.sol";

contract BridgeRecoverHarness is Bridge {
    constructor(
        bytes memory processor,
        bytes[] memory listeners,
        uint16 neededListeners,
        bytes[] memory approvers,
        uint16 neededApprovers
    ) Bridge(processor, listeners, neededListeners, approvers, neededApprovers) {}

    function recoverSigner(
        bytes32 payloadHash,
        bytes calldata signature
    ) external pure returns (address) {
        return _recoverSigner(payloadHash, signature);
    }

    function validatorAddress(bytes memory key) external view returns (address) {
        return _validatorAddress(key);
    }
}

contract BridgeTest is Test {
    event FundsReceived(uint64 indexed eventId, address indexed sender, uint256 amount);
    event Signed(uint64 eventId, address indexed sender, uint64 actionId);
    bytes constant TEST_VALIDATOR_KEY =
        hex"038318535b54105d4a7aae60c08fc45f9687181b4fdfc625bd1a753fa7397fed75";
    bytes constant TEST_VALIDATOR_KEY_2 =
        hex"02ba5734d8f7091719471e7f7ed6b9df170dc70cc661ca05e688601ad984f068b0";
    bytes constant TEST_VALIDATOR_KEY_3 =
        hex"039d9031e97dd78ff8c15aa86939de9b1e791066a0224e331bc962a2099a7b1f04";
    bytes constant TEST_INVALID_PREFIX_KEY =
        hex"041111111111111111111111111111111111111111111111111111111111111111";
    uint256 constant PROCESSOR_PRIVATE_KEY =
        0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80;
    uint256 constant APPROVER_PRIVATE_KEY =
        0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d;
    uint256 constant NON_APPROVER_PRIVATE_KEY =
        0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a;

    Bridge public bridge;
    BridgeRecoverHarness public recoverHarness;
    address public nonAdmin = address(0xB0B);

    function setUp() public {
        bytes[] memory listeners = new bytes[](1);
        listeners[0] = TEST_VALIDATOR_KEY;

        bytes[] memory approvers = new bytes[](1);
        approvers[0] = TEST_VALIDATOR_KEY_2;

        bridge = new Bridge(TEST_VALIDATOR_KEY, listeners, 1, approvers, 1);
        recoverHarness = new BridgeRecoverHarness(
            TEST_VALIDATOR_KEY,
            listeners,
            1,
            approvers,
            1
        );
    }

    function test_ReceiveEth() public {
        vm.deal(nonAdmin, 1 ether);

        vm.prank(nonAdmin);
        (bool ok,) = address(bridge).call{value: 0.25 ether}("");
        assertTrue(ok);
        assertEq(address(bridge).balance, 0.25 ether);
    }

    function test_ReceiveEthEmitsEvent() public {
        vm.deal(nonAdmin, 1 ether);

        vm.expectEmit(true, true, false, true, address(bridge));
        emit FundsReceived(0, nonAdmin, 0.1 ether);

        vm.prank(nonAdmin);
        (bool ok,) = address(bridge).call{value: 0.1 ether}("");
        assertTrue(ok);
    }

    function test_ReceiveEthIncrementsNextEventId() public {
        vm.deal(nonAdmin, 1 ether);
        vm.prank(nonAdmin);
        (bool ok,) = address(bridge).call{value: 0.1 ether}("");
        assertTrue(ok);

        (,,,,, uint64 configNextEventId,) = bridge.get_config();
        assertEq(configNextEventId, 1);
    }

    function test_ExecuteSignedIncrementsIdsAndEmitsEvent() public {
        bytes memory payload = abi.encode(uint64(0), bytes("noop"));
        bytes memory processorSig = _signPayload(PROCESSOR_PRIVATE_KEY, payload);
        bytes[] memory approverSigs = new bytes[](1);
        approverSigs[0] = _signPayload(APPROVER_PRIVATE_KEY, payload);

        vm.expectEmit(true, false, false, true, address(bridge));
        emit Signed(0, nonAdmin, 0);

        vm.prank(nonAdmin);
        bridge.execute(payload, processorSig, approverSigs);

        (,,,,, uint64 configNextEventId, uint64 configNextActionId) = bridge.get_config();
        assertEq(configNextEventId, 1);
        assertEq(configNextActionId, 1);
    }

    function test_RevertWhenExecuteActionIdIncorrect() public {
        bytes memory payload = abi.encode(uint64(1), bytes("noop"));
        bytes memory processorSig = _signPayload(PROCESSOR_PRIVATE_KEY, payload);
        bytes[] memory approverSigs = new bytes[](1);
        approverSigs[0] = _signPayload(APPROVER_PRIVATE_KEY, payload);

        vm.expectRevert(
            abi.encodeWithSelector(Bridge.IncorrectActionId.selector, uint64(0), uint64(1))
        );
        bridge.execute(payload, processorSig, approverSigs);
    }

    function test_RevertWhenProcessorSignatureInvalid() public {
        bytes memory payload = abi.encode(uint64(0), bytes("noop"));
        bytes memory processorSig = _signPayload(NON_APPROVER_PRIVATE_KEY, payload);
        bytes[] memory approverSigs = new bytes[](1);
        approverSigs[0] = _signPayload(APPROVER_PRIVATE_KEY, payload);

        address expected = vm.addr(PROCESSOR_PRIVATE_KEY);
        address received = vm.addr(NON_APPROVER_PRIVATE_KEY);
        vm.expectRevert(
            abi.encodeWithSelector(
                Bridge.InvalidProcessorSignature.selector,
                expected,
                received
            )
        );
        bridge.execute(payload, processorSig, approverSigs);
    }

    function test_RevertWhenApproverSignatureInvalid() public {
        bytes memory payload = abi.encode(uint64(0), bytes("noop"));
        bytes memory processorSig = _signPayload(PROCESSOR_PRIVATE_KEY, payload);
        bytes[] memory approverSigs = new bytes[](1);
        approverSigs[0] = _signPayload(NON_APPROVER_PRIVATE_KEY, payload);

        vm.expectRevert(
            abi.encodeWithSelector(
                Bridge.InvalidApproverSignature.selector,
                vm.addr(NON_APPROVER_PRIVATE_KEY)
            )
        );
        bridge.execute(payload, processorSig, approverSigs);
    }

    function test_RevertWhenApproverSignatureDuplicated() public {
        bytes memory payload = abi.encode(uint64(0), bytes("noop"));
        bytes memory processorSig = _signPayload(PROCESSOR_PRIVATE_KEY, payload);
        bytes memory approverSig = _signPayload(APPROVER_PRIVATE_KEY, payload);
        bytes[] memory approverSigs = new bytes[](2);
        approverSigs[0] = approverSig;
        approverSigs[1] = approverSig;

        vm.expectRevert(
            abi.encodeWithSelector(
                Bridge.DuplicateApproverSignature.selector,
                vm.addr(APPROVER_PRIVATE_KEY)
            )
        );
        bridge.execute(payload, processorSig, approverSigs);
    }

    function test_RevertWhenApproverSignatureMissing() public {
        bytes memory payload = abi.encode(uint64(0), bytes("noop"));
        bytes memory processorSig = _signPayload(PROCESSOR_PRIVATE_KEY, payload);
        bytes[] memory approverSigs = new bytes[](0);

        vm.expectRevert(
            abi.encodeWithSelector(
                Bridge.InsufficientApproverSignatures.selector,
                uint16(1),
                uint256(0)
            )
        );
        bridge.execute(payload, processorSig, approverSigs);
    }

    function test_RecoverSigner() public view {
        bytes memory payload = abi.encode(uint64(0), bytes("noop"));
        bytes memory signature = _signPayload(PROCESSOR_PRIVATE_KEY, payload);
        bytes32 payloadHash = sha256(payload);

        address recovered = recoverHarness.recoverSigner(payloadHash, signature);
        assertEq(recovered, vm.addr(PROCESSOR_PRIVATE_KEY));
    }

    function test_RecoverSignerRevertsWhenSignatureLengthInvalid() public {
        bytes32 payloadHash = sha256(abi.encode(uint64(0), bytes("noop")));
        bytes memory signature = hex"0102";

        vm.expectRevert(
            abi.encodeWithSelector(
                Bridge.InvalidSignatureLength.selector,
                signature.length
            )
        );
        recoverHarness.recoverSigner(payloadHash, signature);
    }

    function test_RecoverSignerRevertsWhenVInvalid() public {
        bytes memory payload = abi.encode(uint64(0), bytes("noop"));
        bytes memory signature = _signPayload(PROCESSOR_PRIVATE_KEY, payload);
        signature[64] = 0x00;
        bytes32 payloadHash = sha256(payload);

        vm.expectRevert(
            abi.encodeWithSelector(Bridge.InvalidSignatureV.selector, uint8(0))
        );
        recoverHarness.recoverSigner(payloadHash, signature);
    }

    function test_ValidatorAddressDerivation() public view {
        address derived = recoverHarness.validatorAddress(TEST_VALIDATOR_KEY);
        assertEq(derived, vm.addr(PROCESSOR_PRIVATE_KEY));
    }

    function test_ValidatorAddressRevertsWhenCurvePointInvalid() public {
        // x == secp256k1 field modulus (p) is invalid and should be rejected.
        bytes memory invalidKey = hex"02fffffffffffffffffffffffffffffffffffffffffffffffffffffffefffffc2f";

        vm.expectRevert(
            abi.encodeWithSelector(Bridge.InvalidCurvePoint.selector, invalidKey)
        );
        recoverHarness.validatorAddress(invalidKey);
    }

    function testFuzz_RecoverSignerMatchesValidatorAddress(
        bytes memory actionData
    ) public view {
        bytes memory payload = abi.encode(uint64(0), actionData);
        bytes memory signature = _signPayload(PROCESSOR_PRIVATE_KEY, payload);
        bytes32 payloadHash = sha256(payload);

        address fromSignature = recoverHarness.recoverSigner(payloadHash, signature);
        address fromValidatorKey = recoverHarness.validatorAddress(TEST_VALIDATOR_KEY);

        assertEq(fromSignature, fromValidatorKey);
    }

    function test_GetConfigReturnsInitializedState() public view {
        (
            bytes memory processor,
            bytes[] memory listeners,
            uint16 neededListeners,
            bytes[] memory approvers,
            uint16 neededApprovers,
            uint64 configNextEventId,
            uint64 configNextActionId
        ) = bridge.get_config();

        assertEq(processor, TEST_VALIDATOR_KEY);
        assertEq(listeners.length, 1);
        assertEq(listeners[0], TEST_VALIDATOR_KEY);
        assertEq(neededListeners, 1);
        assertEq(approvers.length, 1);
        assertEq(approvers[0], TEST_VALIDATOR_KEY_2);
        assertEq(neededApprovers, 1);
        assertEq(configNextEventId, 0);
        assertEq(configNextActionId, 0);
    }

    function test_RevertWhenProcessorKeyHasInvalidLength() public {
        bytes memory shortKey = hex"0211111111111111111111111111111111111111111111111111111111111111";
        bytes[] memory listeners = new bytes[](1);
        listeners[0] = TEST_VALIDATOR_KEY;
        bytes[] memory approvers = new bytes[](1);
        approvers[0] = TEST_VALIDATOR_KEY_2;

        vm.expectRevert(
            abi.encodeWithSelector(Bridge.InvalidProcessorKey.selector, shortKey)
        );
        new Bridge(shortKey, listeners, 1, approvers, 1);
    }

    function test_RevertWhenProcessorKeyHasInvalidPrefix() public {
        bytes[] memory listeners = new bytes[](1);
        listeners[0] = TEST_VALIDATOR_KEY;
        bytes[] memory approvers = new bytes[](1);
        approvers[0] = TEST_VALIDATOR_KEY_2;

        vm.expectRevert(
            abi.encodeWithSelector(
                Bridge.InvalidProcessorKey.selector,
                TEST_INVALID_PREFIX_KEY
            )
        );
        new Bridge(TEST_INVALID_PREFIX_KEY, listeners, 1, approvers, 1);
    }

    function test_RevertWhenListenerKeyInvalid() public {
        bytes[] memory listeners = new bytes[](1);
        listeners[0] = TEST_INVALID_PREFIX_KEY;
        bytes[] memory approvers = new bytes[](1);
        approvers[0] = TEST_VALIDATOR_KEY_2;

        vm.expectRevert(
            abi.encodeWithSelector(
                Bridge.InvalidValidatorKey.selector,
                uint256(0),
                TEST_INVALID_PREFIX_KEY
            )
        );
        new Bridge(TEST_VALIDATOR_KEY, listeners, 1, approvers, 1);
    }

    function test_RevertWhenApproverKeyInvalid() public {
        bytes[] memory listeners = new bytes[](1);
        listeners[0] = TEST_VALIDATOR_KEY;
        bytes[] memory approvers = new bytes[](1);
        approvers[0] = TEST_INVALID_PREFIX_KEY;

        vm.expectRevert(
            abi.encodeWithSelector(
                Bridge.InvalidValidatorKey.selector,
                uint256(0),
                TEST_INVALID_PREFIX_KEY
            )
        );
        new Bridge(TEST_VALIDATOR_KEY_2, listeners, 1, approvers, 1);
    }

    function test_RevertWhenListenerKeysDuplicate() public {
        bytes[] memory listeners = new bytes[](2);
        listeners[0] = TEST_VALIDATOR_KEY;
        listeners[1] = TEST_VALIDATOR_KEY;
        bytes[] memory approvers = new bytes[](1);
        approvers[0] = TEST_VALIDATOR_KEY_2;

        vm.expectRevert(
            abi.encodeWithSelector(
                Bridge.DuplicateValidatorKey.selector,
                uint256(0),
                uint256(1),
                TEST_VALIDATOR_KEY
            )
        );
        new Bridge(TEST_VALIDATOR_KEY_3, listeners, 1, approvers, 1);
    }

    function test_RevertWhenApproverKeysDuplicate() public {
        bytes[] memory listeners = new bytes[](1);
        listeners[0] = TEST_VALIDATOR_KEY;
        bytes[] memory approvers = new bytes[](2);
        approvers[0] = TEST_VALIDATOR_KEY_2;
        approvers[1] = TEST_VALIDATOR_KEY_2;

        vm.expectRevert(
            abi.encodeWithSelector(
                Bridge.DuplicateValidatorKey.selector,
                uint256(0),
                uint256(1),
                TEST_VALIDATOR_KEY_2
            )
        );
        new Bridge(TEST_VALIDATOR_KEY_3, listeners, 1, approvers, 1);
    }

    function test_AllowsCrossRoleKeyReuse() public {
        bytes[] memory listeners2 = new bytes[](1);
        listeners2[0] = TEST_VALIDATOR_KEY;
        bytes[] memory approvers2 = new bytes[](1);
        approvers2[0] = TEST_VALIDATOR_KEY;
        Bridge bridge2 = new Bridge(TEST_VALIDATOR_KEY, listeners2, 1, approvers2, 1);

        (
            bytes memory processor,
            bytes[] memory listeners,
            uint16 neededListeners,
            bytes[] memory approvers,
            uint16 neededApprovers,
            uint64 configNextEventId,
            uint64 configNextActionId
        ) = bridge2.get_config();

        assertEq(processor, TEST_VALIDATOR_KEY);
        assertEq(listeners[0], TEST_VALIDATOR_KEY);
        assertEq(approvers[0], TEST_VALIDATOR_KEY);
        assertEq(neededListeners, 1);
        assertEq(neededApprovers, 1);
        assertEq(configNextEventId, 0);
        assertEq(configNextActionId, 0);
    }

    function test_RevertWhenThirdListenerKeyInvalid_IncludesIndexAndKey() public {
        bytes[] memory listeners = new bytes[](3);
        listeners[0] = TEST_VALIDATOR_KEY;
        listeners[1] = TEST_VALIDATOR_KEY_2;
        listeners[2] = TEST_INVALID_PREFIX_KEY;
        bytes[] memory approvers = new bytes[](1);
        approvers[0] = TEST_VALIDATOR_KEY_3;

        vm.expectRevert(
            abi.encodeWithSelector(
                Bridge.InvalidValidatorKey.selector,
                uint256(2),
                TEST_INVALID_PREFIX_KEY
            )
        );
        new Bridge(TEST_VALIDATOR_KEY, listeners, 1, approvers, 1);
    }

    function _signPayload(
        uint256 privateKey,
        bytes memory payload
    ) internal pure returns (bytes memory) {
        bytes32 hash = sha256(payload);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(privateKey, hash);
        return abi.encodePacked(r, s, v);
    }
}
