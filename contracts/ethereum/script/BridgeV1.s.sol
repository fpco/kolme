// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.30;

import {Script} from "forge-std/Script.sol";
import {BridgeV1} from "../src/BridgeV1.sol";

contract BridgeV1Script is Script {
    BridgeV1 public bridge;

    function setUp() public {}

    function run() public {
        vm.startBroadcast();

        bridge = new BridgeV1(msg.sender);

        vm.stopBroadcast();
    }
}
