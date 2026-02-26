// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.30;

import {Test} from "forge-std/Test.sol";
import {BridgeV1} from "../src/BridgeV1.sol";

contract BridgeV1Test is Test {
    event FundsReceived(address indexed sender, uint256 amount);
    bytes constant TEST_VALIDATOR_KEY =
        hex"021111111111111111111111111111111111111111111111111111111111111111";

    BridgeV1 public bridge;
    address public admin = address(0xA11CE);
    address public nonAdmin = address(0xB0B);

    function setUp() public {
        bytes[] memory listeners = new bytes[](1);
        listeners[0] = TEST_VALIDATOR_KEY;

        bytes[] memory approvers = new bytes[](1);
        approvers[0] = TEST_VALIDATOR_KEY;

        bridge = new BridgeV1(
            admin,
            TEST_VALIDATOR_KEY,
            listeners,
            1,
            approvers,
            1,
            0,
            0
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
        emit FundsReceived(nonAdmin, 0.1 ether);

        vm.prank(nonAdmin);
        (bool ok,) = address(bridge).call{value: 0.1 ether}("");
        assertTrue(ok);
    }

    function test_NonAdminCannotCallAdminFunction() public {
        vm.prank(nonAdmin);
        vm.expectRevert();
        bridge.adminPing();
    }
}
