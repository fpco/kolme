// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.30;

import {Test} from "forge-std/Test.sol";
import {BridgeV1} from "../src/BridgeV1.sol";

contract BridgeV1Test is Test {
    event FundsReceived(address indexed sender, uint256 amount);

    BridgeV1 public bridge;
    address public admin = address(0xA11CE);
    address public nonAdmin = address(0xB0B);

    function setUp() public {
        bridge = new BridgeV1(admin);
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
