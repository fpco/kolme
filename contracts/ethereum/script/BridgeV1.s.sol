// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.30;

import {Script} from "forge-std/Script.sol";
import {BridgeV1} from "../src/BridgeV1.sol";

contract BridgeV1Script is Script {
    bytes constant DEFAULT_VALIDATOR_KEY =
        hex"021111111111111111111111111111111111111111111111111111111111111111";

    BridgeV1 public bridge;

    function setUp() public {}

    function run() public {
        vm.startBroadcast();

        bytes[] memory listeners = new bytes[](1);
        listeners[0] = DEFAULT_VALIDATOR_KEY;
        bytes[] memory approvers = new bytes[](1);
        approvers[0] = DEFAULT_VALIDATOR_KEY;

        bridge = new BridgeV1(
            msg.sender,
            DEFAULT_VALIDATOR_KEY,
            listeners,
            1,
            approvers,
            1,
            0,
            0
        );

        vm.stopBroadcast();
    }
}
