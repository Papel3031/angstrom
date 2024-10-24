// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";

import {SafeTransferLib} from "solady/src/utils/SafeTransferLib.sol";
import {UniV4Inspector} from "test/_view-ext/UniV4Inspector.sol";
import {OpenAngstrom} from "test/_mocks/OpenAngstrom.sol";
import {PoolGate} from "test/_helpers/PoolGate.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {AngstromHandler, Env} from "./AngstromHandler.sol";

import {TickMath} from "v4-core/src/libraries/TickMath.sol";

/// @author philogy <https://github.com/philogy>
contract AngstromInvariantsTest is BaseTest {
    using SafeTransferLib for *;

    uint256 internal TOTAL_ASSETS = 40;

    Env e;
    AngstromHandler handler;

    bytes4[] selectors;

    function setUp() public {
        e.owner = makeAddr("owner");
        e.controller = makeAddr("controller");
        e.feeMaster = makeAddr("feeMaster");

        vm.prank(e.owner);
        e.uniV4 = new UniV4Inspector();
        e.gate = new PoolGate(address(e.uniV4));
        e.angstrom = OpenAngstrom(
            deployAngstrom(type(OpenAngstrom).creationCode, e.uniV4, e.controller, e.feeMaster)
        );
        e.assets = _fillAssets(new MockERC20[](TOTAL_ASSETS));
        e.mirrors = _fillAssets(new MockERC20[](TOTAL_ASSETS));

        handler = new AngstromHandler(e);

        handler.initializePool(0, 1, 60, 0.002e6, TickMath.getSqrtPriceAtTick(0));

        selectors.push(AngstromHandler.doNothing.selector);

        targetSelector(FuzzSelector({addr: address(handler), selectors: selectors}));
        targetContract(address(handler));
    }

    function invariant_bundleSolvency() public view {
        address[] memory assets = handler.enabledAssets();
        for (uint256 i = 0; i < assets.length; i++) {
            address asset = assets[i];
            int256 delta = e.angstrom.getDelta(asset);

            if (delta > 0) {
                assertEq(
                    asset.balanceOf(address(e.angstrom)),
                    handler.ghost_totalHeldBalance(asset) + uint256(delta)
                );
            } else {
                uint256 change;
                unchecked {
                    change = uint256(-delta);
                }
                assertEq(
                    asset.balanceOf(address(e.angstrom)),
                    handler.ghost_totalHeldBalance(asset) - change
                );
            }
        }
    }

    function invariant_ghost_totalHeldConsistency() public view {
        address[] memory assets = handler.enabledAssets();
        address[] memory actors = handler.actors();
        for (uint256 i = 0; i < assets.length; i++) {
            address asset = assets[i];
            uint256 assumedTotal = handler.ghost_totalHeldBalance(asset);
            uint256 realBalance = 0;
            for (uint256 j = 0; j < actors.length; j++) {
                realBalance += e.angstrom.balanceOf(asset, actors[i]);
            }
            assertEq(realBalance, assumedTotal);
        }
    }

    function _fillAssets(MockERC20[] memory assets) internal returns (MockERC20[] memory) {
        for (uint256 i = 0; i < assets.length; i++) {
            MockERC20 newAsset = new MockERC20();
            for (uint256 j = 0; j < i; j++) {
                if (newAsset < assets[j]) {
                    (newAsset, assets[j]) = (assets[j], newAsset);
                }
            }
            assets[i] = newAsset;
        }
        return assets;
    }
}