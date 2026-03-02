// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.30;

import {Test} from "forge-std/Test.sol";
import {Bridge} from "../src/Bridge.sol";

contract BridgeTest is Test {
    event FundsReceived(uint64 eventId, address indexed sender, uint256 amount);
    bytes constant TEST_VALIDATOR_KEY =
        hex"021111111111111111111111111111111111111111111111111111111111111111";
    bytes constant TEST_VALIDATOR_KEY_2 =
        hex"032222222222222222222222222222222222222222222222222222222222222222";
    bytes constant TEST_VALIDATOR_KEY_3 =
        hex"023333333333333333333333333333333333333333333333333333333333333333";
    bytes constant TEST_INVALID_PREFIX_KEY =
        hex"041111111111111111111111111111111111111111111111111111111111111111";

    Bridge public bridge;
    address public admin = address(0xA11CE);
    address public nonAdmin = address(0xB0B);

    function setUp() public {
        bytes[] memory listeners = new bytes[](1);
        listeners[0] = TEST_VALIDATOR_KEY;

        bytes[] memory approvers = new bytes[](1);
        approvers[0] = TEST_VALIDATOR_KEY;

        bridge = new Bridge(
            admin,
            TEST_VALIDATOR_KEY,
            listeners,
            1,
            approvers,
            1
        );
    }

    function test_AdminRoleAssigned() public view {
        assertTrue(bridge.hasRole(bridge.DEFAULT_ADMIN_ROLE(), admin));
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

        vm.expectEmit(true, false, false, true, address(bridge));
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

    function test_NonAdminCannotCallAdminFunction() public {
        vm.prank(nonAdmin);
        vm.expectRevert();
        bridge.adminPing();
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
        assertEq(approvers[0], TEST_VALIDATOR_KEY);
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

        vm.expectRevert("Bridge: invalid processor key");
        new Bridge(admin, shortKey, listeners, 1, approvers, 1);
    }

    function test_RevertWhenProcessorKeyHasInvalidPrefix() public {
        bytes[] memory listeners = new bytes[](1);
        listeners[0] = TEST_VALIDATOR_KEY;
        bytes[] memory approvers = new bytes[](1);
        approvers[0] = TEST_VALIDATOR_KEY_2;

        vm.expectRevert("Bridge: invalid processor key");
        new Bridge(admin, TEST_INVALID_PREFIX_KEY, listeners, 1, approvers, 1);
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
        new Bridge(admin, TEST_VALIDATOR_KEY, listeners, 1, approvers, 1);
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
        new Bridge(admin, TEST_VALIDATOR_KEY_2, listeners, 1, approvers, 1);
    }

    function test_RevertWhenListenerKeysDuplicate() public {
        bytes[] memory listeners = new bytes[](2);
        listeners[0] = TEST_VALIDATOR_KEY;
        listeners[1] = TEST_VALIDATOR_KEY;
        bytes[] memory approvers = new bytes[](1);
        approvers[0] = TEST_VALIDATOR_KEY_2;

        vm.expectRevert("Bridge: duplicate listener key");
        new Bridge(admin, TEST_VALIDATOR_KEY_3, listeners, 1, approvers, 1);
    }

    function test_RevertWhenApproverKeysDuplicate() public {
        bytes[] memory listeners = new bytes[](1);
        listeners[0] = TEST_VALIDATOR_KEY;
        bytes[] memory approvers = new bytes[](2);
        approvers[0] = TEST_VALIDATOR_KEY_2;
        approvers[1] = TEST_VALIDATOR_KEY_2;

        vm.expectRevert("Bridge: duplicate approver key");
        new Bridge(admin, TEST_VALIDATOR_KEY_3, listeners, 1, approvers, 1);
    }

    function test_AllowsCrossRoleKeyReuse() public view {
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
        new Bridge(admin, TEST_VALIDATOR_KEY, listeners, 1, approvers, 1);
    }
}
