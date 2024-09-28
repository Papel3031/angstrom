// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {ExtAngstrom} from "test/_view-ext/ExtAngstrom.sol";
import {NodeManager} from "src/modules/NodeManager.sol";
import {
    PoolConfigStore,
    MAX_FEE,
    PoolConfigStoreLib,
    StoreKey
} from "src/libraries/pool-config/PoolConfigStore.sol";
import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract PoolConfigStoreTest is BaseTest {
    ExtAngstrom angstrom;
    address controller;

    uint256 constant TOTAL_ASSETS = 32;
    address[TOTAL_ASSETS] assets;

    function setUp() public {
        controller = makeAddr("controller");
        angstrom = ExtAngstrom(
            deployAngstrom(type(ExtAngstrom).creationCode, IPoolManager(address(0)), controller)
        );

        assets[0] = makeAddr("asset_0");
        for (uint256 i = 1; i < TOTAL_ASSETS; i++) {
            assets[i] = addrInc(assets[i - 1]);
        }
    }

    function test_default_store() public view {
        PoolConfigStore store = angstrom.configStore();
        assertEq(store.totalEntries(), 0);
    }

    function test_fuzzing_nonControllerCannotConfigure(
        address configurer,
        address asset0,
        address asset1,
        uint16 tickSpacing,
        uint24 feeInE6
    ) public {
        vm.assume(configurer != controller);
        vm.assume(asset0 != asset1);
        tickSpacing = uint16(bound(tickSpacing, 1, type(uint16).max));
        feeInE6 = uint24(bound(feeInE6, 0, MAX_FEE));
        vm.prank(configurer);
        vm.expectRevert(NodeManager.NotController.selector);
        if (asset0 > asset1) (asset0, asset1) = (asset1, asset0);
        angstrom.configurePool(asset0, asset1, tickSpacing, feeInE6);
    }

    function test_configure_1() public {
        vm.prank(controller);
        angstrom.configurePool(assets[0], assets[1], 12, 0.01e6);

        PoolConfigStore store = angstrom.configStore();
        assertEq(store.totalEntries(), 1);
        (int24 tickSpacing, uint24 feeInE6) = store.get(skey(assets[0], assets[1]), 0);
        assertEq(tickSpacing, 12);
        assertEq(feeInE6, 0.01e6);
    }

    function test_configure_newOnly() public {
        vm.prank(controller);
        angstrom.configurePool(assets[0], assets[1], 19, 0.01e6);
        PoolConfigStore store1 = angstrom.configStore();
        assertEq(store1.totalEntries(), 1);
        (int24 tickSpacing, uint24 feeInE6) = store1.get(skey(assets[0], assets[1]), 0);
        assertEq(tickSpacing, 19);
        assertEq(feeInE6, 0.01e6);

        vm.prank(controller);
        angstrom.configurePool(assets[3], assets[31], 120, 0.000134e6);
        PoolConfigStore store2 = angstrom.configStore();
        assertTrue(PoolConfigStore.unwrap(store1) != PoolConfigStore.unwrap(store2));
        assertEq(store2.totalEntries(), 2);
        (tickSpacing, feeInE6) = store2.get(skey(assets[0], assets[1]), 0);
        assertEq(tickSpacing, 19);
        assertEq(feeInE6, 0.01e6);
        (tickSpacing, feeInE6) = store2.get(skey(assets[3], assets[31]), 1);
        assertEq(tickSpacing, 120);
        assertEq(feeInE6, 0.000134e6);

        vm.prank(controller);
        angstrom.configurePool(assets[4], assets[7], 41, 0.1003e6);
        PoolConfigStore store3 = angstrom.configStore();
        assertTrue(PoolConfigStore.unwrap(store2) != PoolConfigStore.unwrap(store3));
        assertEq(store3.totalEntries(), 3);
        (tickSpacing, feeInE6) = store3.get(skey(assets[0], assets[1]), 0);
        assertEq(tickSpacing, 19);
        assertEq(feeInE6, 0.01e6);
        (tickSpacing, feeInE6) = store3.get(skey(assets[3], assets[31]), 1);
        assertEq(tickSpacing, 120);
        assertEq(feeInE6, 0.000134e6);
        (tickSpacing, feeInE6) = store3.get(skey(assets[4], assets[7]), 2);
        assertEq(tickSpacing, 41);
        assertEq(feeInE6, 0.1003e6);
    }

    function addrInc(address prev) internal pure returns (address next) {
        assembly ("memory-safe") {
            mstore(0x00, prev)
            let hash := keccak256(0x00, 0x20)
            next := add(prev, shr(120, hash))
        }
    }

    function skey(address asset0, address asset1) internal pure returns (StoreKey key) {
        assertTrue(asset0 < asset1, "Building key with out of order assets");
        key = PoolConfigStoreLib.keyFromAssetsUnchecked(asset0, asset1);
    }
}
