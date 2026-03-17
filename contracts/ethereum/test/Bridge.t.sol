// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.30;

import {Test} from "forge-std/Test.sol";
import {Bridge} from "../src/Bridge.sol";
import {BridgeActions} from "../src/BridgeActions.sol";
import {BridgeBase} from "../src/BridgeBase.sol";

contract MockERC20 {
    mapping(address => uint256) internal balances;
    mapping(address => mapping(address => uint256)) internal allowances;

    function mint(address to, uint256 amount) external {
        balances[to] += amount;
    }

    function balanceOf(address account) external view returns (uint256) {
        return balances[account];
    }

    function allowance(
        address owner,
        address spender
    ) external view returns (uint256) {
        return allowances[owner][spender];
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowances[msg.sender][spender] = amount;
        return true;
    }

    function transferFrom(
        address from,
        address to,
        uint256 amount
    ) external virtual returns (bool) {
        uint256 allowed = allowances[from][msg.sender];
        if (allowed < amount || balances[from] < amount) {
            return false;
        }
        allowances[from][msg.sender] = allowed - amount;
        balances[from] -= amount;
        balances[to] += amount;
        return true;
    }
}

contract MockERC20TransferFromFalse is MockERC20 {
    function transferFrom(
        address,
        address,
        uint256
    ) external pure override returns (bool) {
        return false;
    }
}

contract RevertingReceiver {
    receive() external payable {
        revert("reject");
    }
}

// A contract designed to be called from bridge's execute_signed() entry point and
// calling it back, triggering reentrancy guard, and ensuring that expected error was hit
contract ReentrantExecuteSignedCaller {
    Bridge private immutable BRIDGE;
    bytes4 private constant REENTRANCY_GUARD_SELECTOR =
        bytes4(keccak256("ReentrancyGuardReentrantCall()"));

    constructor(Bridge bridge_) {
        BRIDGE = bridge_;
    }

    function reenter() external returns (bool) {
        bytes[] memory emptyApprovers = new bytes[](0);
        (bool ok, bytes memory ret) = address(BRIDGE).call(
            abi.encodeWithSelector(
                Bridge.execute_signed.selector,
                bytes(""),
                bytes(""),
                emptyApprovers
            )
        );

        if (ok) {
            revert("expected revert");
        }

        // check that the failure is reentrancy guard error
        if (ret.length < 4) {
            revert("missing selector");
        }

        bytes4 selector;
        assembly {
            // first 32 bytes of ret's memory storage is its length
            selector := mload(add(ret, 0x20))
        }
        if (selector != REENTRANCY_GUARD_SELECTOR) {
            revert("unexpected selector");
        }
        return true;
    }
}

contract BridgeRecoverHarness is Bridge {
    constructor(
        bytes memory processor,
        bytes[] memory listeners,
        uint16 neededListeners,
        bytes[] memory approvers,
        uint16 neededApprovers
    )
        Bridge(
            processor,
            listeners,
            neededListeners,
            approvers,
            neededApprovers
        )
    {}

    function recoverSigner(
        bytes32 payloadHash,
        bytes calldata signature
    ) external pure returns (address) {
        return _recoverSigner(payloadHash, signature);
    }

    function validatorAddress(
        bytes memory key
    ) external view returns (address) {
        return _validatorAddress(key);
    }
}

contract BridgeTest is Test {
    // Keep these in sync with action constants in BridgeActions.sol.
    uint8 constant ACTION_EXECUTE = 0;
    uint8 constant ACTION_SELF_REPLACE = 1;
    uint8 constant ACTION_NEW_SET = 2;

    event FundsReceived(
        uint64 indexed eventId,
        address indexed sender,
        address[] tokens,
        uint256[] amounts
    );
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
        (bool ok, ) = address(bridge).call{value: 0.25 ether}("");
        assertTrue(ok);
        assertEq(address(bridge).balance, 0.25 ether);
    }

    function test_ReceiveEthEmitsEvent() public {
        vm.deal(nonAdmin, 1 ether);
        address[] memory tokens = new address[](1);
        uint256[] memory amounts = new uint256[](1);
        tokens[0] = address(0);
        amounts[0] = 0.1 ether;

        vm.expectEmit(true, true, false, true, address(bridge));
        emit FundsReceived(1, nonAdmin, tokens, amounts);

        vm.prank(nonAdmin);
        (bool ok, ) = address(bridge).call{value: 0.1 ether}("");
        assertTrue(ok);
    }

    function test_ReceiveEthIncrementsNextEventId() public {
        vm.deal(nonAdmin, 1 ether);
        vm.prank(nonAdmin);
        (bool ok, ) = address(bridge).call{value: 0.1 ether}("");
        assertTrue(ok);

        (, , , , , uint64 configNextEventId, ) = bridge.get_config();
        assertEq(configNextEventId, 2);
    }

    function test_RegularErc20EmitsEventAndUpdatesBalances() public {
        MockERC20 token = new MockERC20();
        token.mint(nonAdmin, 100);

        vm.prank(nonAdmin);
        assertTrue(token.approve(address(bridge), 70));

        address[] memory tokens = new address[](1);
        uint256[] memory amounts = new uint256[](1);
        tokens[0] = address(token);
        amounts[0] = 70;
        bytes[] memory keys = new bytes[](0);
        vm.expectEmit(true, true, false, true, address(bridge));
        emit FundsReceived(1, nonAdmin, tokens, amounts);

        vm.prank(nonAdmin);
        bridge.regular(tokens, amounts, keys);

        assertEq(token.balanceOf(address(bridge)), 70);
        assertEq(token.balanceOf(nonAdmin), 30);

        (, , , , , uint64 configNextEventId, ) = bridge.get_config();
        assertEq(configNextEventId, 2);
    }

    function test_RevertWhenRegularDepositTokenZero() public {
        address[] memory tokens = new address[](1);
        uint256[] memory amounts = new uint256[](1);
        tokens[0] = address(0);
        amounts[0] = 1;
        bytes[] memory keys = new bytes[](0);
        vm.expectRevert(
            abi.encodeWithSelector(
                Bridge.InvalidDepositToken.selector,
                address(0)
            )
        );
        bridge.regular(tokens, amounts, keys);
    }

    function test_RevertWhenRegularAllowanceInsufficient() public {
        MockERC20 token = new MockERC20();
        token.mint(nonAdmin, 100);
        address[] memory tokens = new address[](1);
        uint256[] memory amounts = new uint256[](1);
        tokens[0] = address(token);
        amounts[0] = 10;
        bytes[] memory keys = new bytes[](0);

        vm.expectRevert(
            abi.encodeWithSelector(
                Bridge.InsufficientAllowance.selector,
                address(token),
                nonAdmin,
                uint256(0),
                uint256(10)
            )
        );
        vm.prank(nonAdmin);
        bridge.regular(tokens, amounts, keys);
    }

    function test_RevertWhenRegularTransferFromFails() public {
        MockERC20TransferFromFalse token = new MockERC20TransferFromFalse();
        token.mint(nonAdmin, 100);

        vm.prank(nonAdmin);
        assertTrue(token.approve(address(bridge), 10));

        address[] memory tokens = new address[](1);
        uint256[] memory amounts = new uint256[](1);
        tokens[0] = address(token);
        amounts[0] = 10;
        bytes[] memory keys = new bytes[](0);
        vm.expectRevert(
            abi.encodeWithSelector(
                Bridge.ERC20TransferFailed.selector,
                address(token),
                nonAdmin,
                uint256(10)
            )
        );
        vm.prank(nonAdmin);
        bridge.regular(tokens, amounts, keys);
    }

    function test_RevertWhenRegularFundsLengthMismatch() public {
        address[] memory tokens = new address[](2);
        uint256[] memory amounts = new uint256[](1);
        tokens[0] = address(1);
        tokens[1] = address(2);
        amounts[0] = 1;
        bytes[] memory keys = new bytes[](0);

        vm.expectRevert(
            abi.encodeWithSelector(
                Bridge.RegularFundsLengthMismatch.selector,
                uint256(2),
                uint256(1)
            )
        );
        bridge.regular(tokens, amounts, keys);
    }

    function test_RegularErc20BatchTransfers() public {
        MockERC20 tokenA = new MockERC20();
        MockERC20 tokenB = new MockERC20();
        tokenA.mint(nonAdmin, 100);
        tokenB.mint(nonAdmin, 50);

        vm.startPrank(nonAdmin);
        assertTrue(tokenA.approve(address(bridge), 40));
        assertTrue(tokenB.approve(address(bridge), 20));
        vm.stopPrank();

        address[] memory tokens = new address[](2);
        uint256[] memory amounts = new uint256[](2);
        tokens[0] = address(tokenA);
        tokens[1] = address(tokenB);
        amounts[0] = 40;
        amounts[1] = 20;
        bytes[] memory keys = new bytes[](0);

        vm.expectEmit(true, true, false, true, address(bridge));
        emit FundsReceived(1, nonAdmin, tokens, amounts);

        vm.prank(nonAdmin);
        bridge.regular(tokens, amounts, keys);

        assertEq(tokenA.balanceOf(address(bridge)), 40);
        assertEq(tokenB.balanceOf(address(bridge)), 20);
    }

    function test_ExecuteSignedIncrementsIdsAndEmitsEvent() public {
        bytes memory actionData = abi.encode(
            ACTION_EXECUTE,
            abi.encode(nonAdmin, 0, bytes(""))
        );
        bytes memory payload = abi.encode(uint64(0), actionData);
        bytes memory processorSig = _signPayload(
            PROCESSOR_PRIVATE_KEY,
            payload
        );
        bytes[] memory approverSigs = new bytes[](1);
        approverSigs[0] = _signPayload(APPROVER_PRIVATE_KEY, payload);

        vm.expectEmit(true, true, false, true, address(bridge));
        emit Signed(1, nonAdmin, 0);

        vm.prank(nonAdmin);
        bridge.execute_signed(payload, processorSig, approverSigs);

        (, , , , , uint64 configNextEventId, uint64 configNextActionId) = bridge
            .get_config();
        assertEq(configNextEventId, 2);
        assertEq(configNextActionId, 1);
    }

    function test_RevertWhenExecuteActionIdIncorrect() public {
        bytes memory actionData = abi.encode(uint8(255), bytes("noop"));
        bytes memory payload = abi.encode(uint64(1), actionData);
        bytes memory processorSig = _signPayload(
            PROCESSOR_PRIVATE_KEY,
            payload
        );
        bytes[] memory approverSigs = new bytes[](1);
        approverSigs[0] = _signPayload(APPROVER_PRIVATE_KEY, payload);

        vm.expectRevert(
            abi.encodeWithSelector(
                Bridge.IncorrectActionId.selector,
                uint64(0),
                uint64(1)
            )
        );
        bridge.execute_signed(payload, processorSig, approverSigs);
    }

    function test_RevertWhenProcessorSignatureInvalid() public {
        bytes memory actionData = abi.encode(uint8(255), bytes("noop"));
        bytes memory payload = abi.encode(uint64(0), actionData);
        bytes memory processorSig = _signPayload(
            NON_APPROVER_PRIVATE_KEY,
            payload
        );
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
        bridge.execute_signed(payload, processorSig, approverSigs);
    }

    function test_RevertWhenApproverSignatureInvalid() public {
        bytes memory actionData = abi.encode(uint8(255), bytes("noop"));
        bytes memory payload = abi.encode(uint64(0), actionData);
        bytes memory processorSig = _signPayload(
            PROCESSOR_PRIVATE_KEY,
            payload
        );
        bytes[] memory approverSigs = new bytes[](1);
        approverSigs[0] = _signPayload(NON_APPROVER_PRIVATE_KEY, payload);

        vm.expectRevert(
            abi.encodeWithSelector(
                BridgeBase.InvalidApproverSignature.selector,
                vm.addr(NON_APPROVER_PRIVATE_KEY)
            )
        );
        bridge.execute_signed(payload, processorSig, approverSigs);
    }

    function test_RevertWhenApproverSignatureDuplicated() public {
        bytes[] memory listeners = new bytes[](1);
        listeners[0] = TEST_VALIDATOR_KEY;
        bytes[] memory configuredApprovers = new bytes[](2);
        configuredApprovers[0] = TEST_VALIDATOR_KEY_2;
        configuredApprovers[1] = TEST_VALIDATOR_KEY_3;
        Bridge bridge2 = new Bridge(
            TEST_VALIDATOR_KEY,
            listeners,
            1,
            configuredApprovers,
            2
        );

        bytes memory actionData = abi.encode(uint8(255), bytes("noop"));
        bytes memory payload = abi.encode(uint64(0), actionData);
        bytes memory processorSig = _signPayload(
            PROCESSOR_PRIVATE_KEY,
            payload
        );
        bytes memory approverSig = _signPayload(APPROVER_PRIVATE_KEY, payload);
        bytes[] memory approverSigs = new bytes[](2);
        approverSigs[0] = approverSig;
        approverSigs[1] = approverSig;

        vm.expectRevert(
            abi.encodeWithSelector(
                BridgeBase.DuplicateApproverSignature.selector,
                vm.addr(APPROVER_PRIVATE_KEY)
            )
        );
        bridge2.execute_signed(payload, processorSig, approverSigs);
    }

    function test_RevertWhenApproverSignatureMissing() public {
        bytes memory actionData = abi.encode(uint8(255), bytes("noop"));
        bytes memory payload = abi.encode(uint64(0), actionData);
        bytes memory processorSig = _signPayload(
            PROCESSOR_PRIVATE_KEY,
            payload
        );
        bytes[] memory approverSigs = new bytes[](0);

        vm.expectRevert(
            abi.encodeWithSelector(
                BridgeBase.InsufficientApproverSignatures.selector,
                uint16(1),
                uint256(0)
            )
        );
        bridge.execute_signed(payload, processorSig, approverSigs);
    }

    function test_RecoverSigner() public view {
        bytes memory actionData = abi.encode(uint8(255), bytes("noop"));
        bytes memory payload = abi.encode(uint64(0), actionData);
        bytes memory signature = _signPayload(PROCESSOR_PRIVATE_KEY, payload);
        bytes32 payloadHash = sha256(payload);

        address recovered = recoverHarness.recoverSigner(
            payloadHash,
            signature
        );
        assertEq(recovered, vm.addr(PROCESSOR_PRIVATE_KEY));
    }

    function test_RecoverSignerRevertsWhenSignatureLengthInvalid() public {
        bytes memory actionData = abi.encode(uint8(255), bytes("noop"));
        bytes32 payloadHash = sha256(abi.encode(uint64(0), actionData));
        bytes memory signature = hex"0102";

        vm.expectRevert(
            abi.encodeWithSelector(
                BridgeBase.InvalidSignatureLength.selector,
                signature.length
            )
        );
        recoverHarness.recoverSigner(payloadHash, signature);
    }

    function test_RecoverSignerRevertsWhenVInvalid() public {
        bytes memory actionData = abi.encode(uint8(255), bytes("noop"));
        bytes memory payload = abi.encode(uint64(0), actionData);
        bytes memory signature = _signPayload(PROCESSOR_PRIVATE_KEY, payload);
        signature[64] = 0x00;
        bytes32 payloadHash = sha256(payload);

        vm.expectRevert(
            abi.encodeWithSelector(
                BridgeBase.InvalidSignatureV.selector,
                uint8(0)
            )
        );
        recoverHarness.recoverSigner(payloadHash, signature);
    }

    function test_ValidatorAddressDerivation() public view {
        address derived = recoverHarness.validatorAddress(TEST_VALIDATOR_KEY);
        assertEq(derived, vm.addr(PROCESSOR_PRIVATE_KEY));
    }

    function test_ValidatorAddressRevertsWhenCurvePointInvalid() public {
        // x == secp256k1 field modulus (p) is invalid and should be rejected.
        bytes
            memory invalidKey = hex"02fffffffffffffffffffffffffffffffffffffffffffffffffffffffefffffc2f";

        vm.expectRevert(
            abi.encodeWithSelector(
                BridgeBase.InvalidCurvePoint.selector,
                invalidKey
            )
        );
        recoverHarness.validatorAddress(invalidKey);
    }

    function testFuzz_RecoverSignerMatchesValidatorAddress(
        bytes memory actionData
    ) public view {
        bytes memory payload = abi.encode(
            uint64(0),
            abi.encode(uint8(255), actionData)
        );
        bytes memory signature = _signPayload(PROCESSOR_PRIVATE_KEY, payload);
        bytes32 payloadHash = sha256(payload);

        address fromSignature = recoverHarness.recoverSigner(
            payloadHash,
            signature
        );
        address fromValidatorKey = recoverHarness.validatorAddress(
            TEST_VALIDATOR_KEY
        );

        assertEq(fromSignature, fromValidatorKey);
    }

    function test_ExecuteSignedTransferEthAction() public {
        vm.deal(address(bridge), 1 ether);
        address recipient = address(0xA11CE);
        uint256 amount = 0.2 ether;

        bytes memory actionData = abi.encode(
            ACTION_EXECUTE,
            abi.encode(recipient, amount, bytes(""))
        );
        bytes memory payload = abi.encode(uint64(0), actionData);
        bytes memory processorSig = _signPayload(
            PROCESSOR_PRIVATE_KEY,
            payload
        );
        bytes[] memory approverSigs = new bytes[](1);
        approverSigs[0] = _signPayload(APPROVER_PRIVATE_KEY, payload);

        uint256 recipientBefore = recipient.balance;
        bridge.execute_signed(payload, processorSig, approverSigs);

        assertEq(recipient.balance, recipientBefore + amount);
        assertEq(address(bridge).balance, 1 ether - amount);
    }

    function test_RevertWhenExecuteActionCallFails() public {
        vm.deal(address(bridge), 1 ether);
        RevertingReceiver receiver = new RevertingReceiver();

        bytes memory callData = abi.encodeWithSignature("doesNotExist()");
        bytes memory actionData = abi.encode(
            ACTION_EXECUTE,
            abi.encode(address(receiver), 0, callData)
        );
        bytes memory payload = abi.encode(uint64(0), actionData);
        bytes memory processorSig = _signPayload(
            PROCESSOR_PRIVATE_KEY,
            payload
        );
        bytes[] memory approverSigs = new bytes[](1);
        approverSigs[0] = _signPayload(APPROVER_PRIVATE_KEY, payload);

        vm.expectRevert(
            abi.encodeWithSelector(
                BridgeActions.ExecuteCallFailed.selector,
                address(receiver),
                uint256(0),
                callData
            )
        );
        bridge.execute_signed(payload, processorSig, approverSigs);
    }

    function test_ExecuteSignedRejectsReentrantExecuteSignedCall() public {
        ReentrantExecuteSignedCaller reentrant = new ReentrantExecuteSignedCaller(
                bridge
            );
        bytes memory actionData = abi.encode(
            ACTION_EXECUTE,
            abi.encode(
                address(reentrant),
                uint256(0),
                abi.encodeWithSignature("reenter()")
            )
        );
        bytes memory payload = abi.encode(uint64(0), actionData);
        bytes memory processorSig = _signPayload(
            PROCESSOR_PRIVATE_KEY,
            payload
        );
        bytes[] memory approverSigs = new bytes[](1);
        approverSigs[0] = _signPayload(APPROVER_PRIVATE_KEY, payload);

        bridge.execute_signed(payload, processorSig, approverSigs);

        (, , , , , uint64 configNextEventId, uint64 configNextActionId) = bridge
            .get_config();
        assertEq(configNextEventId, 2);
        assertEq(configNextActionId, 1);
    }

    function test_ExecuteSignedSelfReplaceProcessorAction() public {
        bytes memory actionData = abi.encode(
            ACTION_SELF_REPLACE,
            abi.encode(uint8(1), TEST_VALIDATOR_KEY, TEST_VALIDATOR_KEY_2)
        );
        bytes memory payload = abi.encode(uint64(0), actionData);
        bytes memory processorSig = _signPayload(APPROVER_PRIVATE_KEY, payload);
        bytes[] memory approverSigs = new bytes[](1);
        approverSigs[0] = _signPayload(APPROVER_PRIVATE_KEY, payload);

        bridge.execute_signed(payload, processorSig, approverSigs);

        (
            bytes memory processor,
            bytes[] memory listeners,
            uint16 neededListeners,
            bytes[] memory approvers,
            uint16 neededApprovers,
            uint64 configNextEventId,
            uint64 configNextActionId
        ) = bridge.get_config();
        assertEq(processor, TEST_VALIDATOR_KEY_2);
        assertEq(listeners.length, 1);
        assertEq(neededListeners, 1);
        assertEq(approvers.length, 1);
        assertEq(neededApprovers, 1);
        assertEq(configNextEventId, 2);
        assertEq(configNextActionId, 1);
    }

    function test_RevertWhenSelfReplaceProcessorSignedByCurrentProcessor()
        public
    {
        bytes memory actionData = abi.encode(
            ACTION_SELF_REPLACE,
            abi.encode(uint8(1), TEST_VALIDATOR_KEY, TEST_VALIDATOR_KEY_2)
        );
        bytes memory payload = abi.encode(uint64(0), actionData);
        bytes memory processorSig = _signPayload(
            PROCESSOR_PRIVATE_KEY,
            payload
        );
        bytes[] memory approverSigs = new bytes[](1);
        approverSigs[0] = _signPayload(APPROVER_PRIVATE_KEY, payload);

        vm.expectRevert(
            abi.encodeWithSelector(
                Bridge.InvalidProcessorSignature.selector,
                vm.addr(APPROVER_PRIVATE_KEY),
                vm.addr(PROCESSOR_PRIVATE_KEY)
            )
        );
        bridge.execute_signed(payload, processorSig, approverSigs);
    }

    function test_ExecuteSignedSelfReplaceApproverActionUsesReplacementApproverSet()
        public
    {
        bytes memory actionData = abi.encode(
            ACTION_SELF_REPLACE,
            abi.encode(uint8(2), TEST_VALIDATOR_KEY_2, TEST_VALIDATOR_KEY)
        );
        bytes memory payload = abi.encode(uint64(0), actionData);
        bytes memory processorSig = _signPayload(
            PROCESSOR_PRIVATE_KEY,
            payload
        );
        bytes[] memory approverSigs = new bytes[](1);
        approverSigs[0] = _signPayload(PROCESSOR_PRIVATE_KEY, payload);

        bridge.execute_signed(payload, processorSig, approverSigs);

        (
            ,
            ,
            ,
            bytes[] memory approvers,
            uint16 neededApprovers,
            uint64 configNextEventId,
            uint64 configNextActionId
        ) = bridge.get_config();
        assertEq(approvers.length, 1);
        assertEq(approvers[0], TEST_VALIDATOR_KEY);
        assertEq(neededApprovers, 1);
        assertEq(configNextEventId, 2);
        assertEq(configNextActionId, 1);
    }

    function test_RevertWhenSelfReplaceApproverSignedByOldApprover() public {
        bytes memory actionData = abi.encode(
            ACTION_SELF_REPLACE,
            abi.encode(uint8(2), TEST_VALIDATOR_KEY_2, TEST_VALIDATOR_KEY)
        );
        bytes memory payload = abi.encode(uint64(0), actionData);
        bytes memory processorSig = _signPayload(
            PROCESSOR_PRIVATE_KEY,
            payload
        );
        bytes[] memory approverSigs = new bytes[](1);
        approverSigs[0] = _signPayload(APPROVER_PRIVATE_KEY, payload);

        vm.expectRevert(
            abi.encodeWithSelector(
                BridgeBase.InvalidApproverSignature.selector,
                vm.addr(APPROVER_PRIVATE_KEY)
            )
        );
        bridge.execute_signed(payload, processorSig, approverSigs);
    }

    function test_ExecuteSignedNewSetAction() public {
        bytes[] memory listeners = new bytes[](1);
        listeners[0] = TEST_VALIDATOR_KEY_3;
        bytes[] memory approvers = new bytes[](1);
        approvers[0] = TEST_VALIDATOR_KEY;
        bytes memory rendered = bytes("new-set-proof");
        bytes[] memory innerApprovals = new bytes[](2);
        innerApprovals[0] = _signPayload(PROCESSOR_PRIVATE_KEY, rendered);
        innerApprovals[1] = _signPayload(APPROVER_PRIVATE_KEY, rendered);

        bytes memory actionData = abi.encode(
            ACTION_NEW_SET,
            abi.encode(
                TEST_VALIDATOR_KEY_2,
                listeners,
                uint16(1),
                approvers,
                uint16(1),
                rendered,
                innerApprovals
            )
        );
        bytes memory payload = abi.encode(uint64(0), actionData);
        bytes memory processorSig = _signPayload(APPROVER_PRIVATE_KEY, payload);
        bytes[] memory approverSigs = new bytes[](1);
        approverSigs[0] = _signPayload(PROCESSOR_PRIVATE_KEY, payload);

        bridge.execute_signed(payload, processorSig, approverSigs);

        (
            bytes memory processor,
            bytes[] memory cfgListeners,
            uint16 neededListeners,
            bytes[] memory cfgApprovers,
            uint16 neededApprovers,
            uint64 configNextEventId,
            uint64 configNextActionId
        ) = bridge.get_config();

        assertEq(processor, TEST_VALIDATOR_KEY_2);
        assertEq(cfgListeners.length, 1);
        assertEq(cfgListeners[0], TEST_VALIDATOR_KEY_3);
        assertEq(neededListeners, 1);
        assertEq(cfgApprovers.length, 1);
        assertEq(cfgApprovers[0], TEST_VALIDATOR_KEY);
        assertEq(neededApprovers, 1);
        assertEq(configNextEventId, 2);
        assertEq(configNextActionId, 1);
    }

    function test_RevertWhenNewSetSignedByCurrentSetNotProposedSet() public {
        bytes[] memory listeners = new bytes[](1);
        listeners[0] = TEST_VALIDATOR_KEY_3;
        bytes[] memory approvers = new bytes[](1);
        approvers[0] = TEST_VALIDATOR_KEY;
        bytes memory rendered = bytes("new-set-proof");
        bytes[] memory innerApprovals = new bytes[](2);
        innerApprovals[0] = _signPayload(PROCESSOR_PRIVATE_KEY, rendered);
        innerApprovals[1] = _signPayload(APPROVER_PRIVATE_KEY, rendered);

        bytes memory actionData = abi.encode(
            ACTION_NEW_SET,
            abi.encode(
                TEST_VALIDATOR_KEY_2,
                listeners,
                uint16(1),
                approvers,
                uint16(1),
                rendered,
                innerApprovals
            )
        );
        bytes memory payload = abi.encode(uint64(0), actionData);
        bytes memory processorSig = _signPayload(
            PROCESSOR_PRIVATE_KEY,
            payload
        );
        bytes[] memory approverSigs = new bytes[](1);
        approverSigs[0] = _signPayload(APPROVER_PRIVATE_KEY, payload);

        vm.expectRevert(
            abi.encodeWithSelector(
                Bridge.InvalidProcessorSignature.selector,
                vm.addr(APPROVER_PRIVATE_KEY),
                vm.addr(PROCESSOR_PRIVATE_KEY)
            )
        );
        bridge.execute_signed(payload, processorSig, approverSigs);
    }

    function test_RevertWhenNewSetApproverSignedByCurrentSetNotProposedSet()
        public
    {
        bytes[] memory listeners = new bytes[](1);
        listeners[0] = TEST_VALIDATOR_KEY_3;
        bytes[] memory approvers = new bytes[](1);
        approvers[0] = TEST_VALIDATOR_KEY;
        bytes memory rendered = bytes("new-set-proof");
        bytes[] memory innerApprovals = new bytes[](2);
        innerApprovals[0] = _signPayload(PROCESSOR_PRIVATE_KEY, rendered);
        innerApprovals[1] = _signPayload(APPROVER_PRIVATE_KEY, rendered);

        bytes memory actionData = abi.encode(
            ACTION_NEW_SET,
            abi.encode(
                TEST_VALIDATOR_KEY_2,
                listeners,
                uint16(1),
                approvers,
                uint16(1),
                rendered,
                innerApprovals
            )
        );
        bytes memory payload = abi.encode(uint64(0), actionData);

        // Proposed processor key is TEST_VALIDATOR_KEY_2, so this part is valid.
        bytes memory processorSig = _signPayload(APPROVER_PRIVATE_KEY, payload);
        bytes[] memory approverSigs = new bytes[](1);
        // Proposed approver key is TEST_VALIDATOR_KEY, but this signature is from old approver.
        approverSigs[0] = _signPayload(APPROVER_PRIVATE_KEY, payload);

        vm.expectRevert(
            abi.encodeWithSelector(
                BridgeBase.InvalidApproverSignature.selector,
                vm.addr(APPROVER_PRIVATE_KEY)
            )
        );
        bridge.execute_signed(payload, processorSig, approverSigs);
    }

    function test_RevertWhenNewSetInnerProofInsufficientGroups() public {
        bytes[] memory listeners = new bytes[](1);
        listeners[0] = TEST_VALIDATOR_KEY_3;
        bytes[] memory approvers = new bytes[](1);
        approvers[0] = TEST_VALIDATOR_KEY;
        bytes memory rendered = bytes("new-set-proof");
        bytes[] memory innerApprovals = new bytes[](1);
        innerApprovals[0] = _signPayload(APPROVER_PRIVATE_KEY, rendered);

        bytes memory actionData = abi.encode(
            ACTION_NEW_SET,
            abi.encode(
                TEST_VALIDATOR_KEY_2,
                listeners,
                uint16(1),
                approvers,
                uint16(1),
                rendered,
                innerApprovals
            )
        );
        bytes memory payload = abi.encode(uint64(0), actionData);
        bytes memory processorSig = _signPayload(APPROVER_PRIVATE_KEY, payload);
        bytes[] memory approverSigs = new bytes[](1);
        approverSigs[0] = _signPayload(PROCESSOR_PRIVATE_KEY, payload);

        vm.expectRevert(
            abi.encodeWithSelector(
                BridgeActions.InsufficientNewSetGroupSignatures.selector,
                uint8(2),
                uint8(1)
            )
        );
        bridge.execute_signed(payload, processorSig, approverSigs);
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
        assertEq(configNextEventId, 1);
        assertEq(configNextActionId, 0);
    }

    function test_RevertWhenProcessorKeyHasInvalidLength() public {
        bytes
            memory shortKey = hex"0211111111111111111111111111111111111111111111111111111111111111";
        bytes[] memory listeners = new bytes[](1);
        listeners[0] = TEST_VALIDATOR_KEY;
        bytes[] memory approvers = new bytes[](1);
        approvers[0] = TEST_VALIDATOR_KEY_2;

        vm.expectRevert(
            abi.encodeWithSelector(
                BridgeBase.InvalidProcessorKey.selector,
                shortKey
            )
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
                BridgeBase.InvalidProcessorKey.selector,
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
                BridgeBase.InvalidValidatorKey.selector,
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
                BridgeBase.InvalidValidatorKey.selector,
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
                BridgeBase.DuplicateValidatorKey.selector,
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
                BridgeBase.DuplicateValidatorKey.selector,
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
        Bridge bridge2 = new Bridge(
            TEST_VALIDATOR_KEY,
            listeners2,
            1,
            approvers2,
            1
        );

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
        assertEq(configNextEventId, 1);
        assertEq(configNextActionId, 0);
    }

    function test_RevertWhenThirdListenerKeyInvalid_IncludesIndexAndKey()
        public
    {
        bytes[] memory listeners = new bytes[](3);
        listeners[0] = TEST_VALIDATOR_KEY;
        listeners[1] = TEST_VALIDATOR_KEY_2;
        listeners[2] = TEST_INVALID_PREFIX_KEY;
        bytes[] memory approvers = new bytes[](1);
        approvers[0] = TEST_VALIDATOR_KEY_3;

        vm.expectRevert(
            abi.encodeWithSelector(
                BridgeBase.InvalidValidatorKey.selector,
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
