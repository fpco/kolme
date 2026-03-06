// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.30;

import {Script} from "forge-std/Script.sol";
import {Bridge} from "../src/Bridge.sol";

contract BridgeScript is Script {
    bytes constant DEFAULT_PROCESSOR_KEY =
        hex"038318535b54105d4a7aae60c08fc45f9687181b4fdfc625bd1a753fa7397fed75";
    bytes constant DEFAULT_APPROVER_KEY =
        hex"02ba5734d8f7091719471e7f7ed6b9df170dc70cc661ca05e688601ad984f068b0";

    Bridge public bridge;

    function setUp() public {}

    function run() public {
        vm.startBroadcast();

        bytes[] memory listeners = new bytes[](1);
        listeners[0] = DEFAULT_PROCESSOR_KEY;
        bytes[] memory approvers = new bytes[](1);
        approvers[0] = DEFAULT_APPROVER_KEY;

        bridge = new Bridge(DEFAULT_PROCESSOR_KEY, listeners, 1, approvers, 1);

        vm.stopBroadcast();
    }
}
