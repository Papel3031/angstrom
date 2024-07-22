// SPDX-License-Identifier: BUSL-1.1
pragma solidity 0.8.24;

import {ERC712} from "./modules/ERC712.sol";
import {NodeManager} from "./modules/NodeManager.sol";
import {Accounter, PoolSwap} from "./modules/Accounter.sol";
import {PoolRewardsManager, PoolRewardsUpdate} from "./modules/PoolRewardsManager.sol";
import {UnorderedNonces} from "./modules/UnorderedNonces.sol";
import {UniConsumer} from "./modules/UniConsumer.sol";

import {Globals} from "./types/Globals.sol";
import {Asset} from "./types/Asset.sol";
import {tuint256} from "transient-goodies/TransientPrimitives.sol";
import {PriceGraphLib, PriceGraph, AssetIndex, Price} from "./types/PriceGraph.sol";
import {GenericOrder, TopOfBlockOrderEnvelope, OrderType, OrderMode} from "./types/OrderTypes.sol";
import {Hooks} from "v4-core/src/libraries/Hooks.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {SET_POOL_FEE, TICK_SPACING} from "./Constants.sol";

import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";
import {DecoderLib} from "./libraries/DecoderLib.sol";
import {FixedPointMathLib} from "solady/src/utils/FixedPointMathLib.sol";
import {SignatureCheckerLib} from "solady/src/utils/SignatureCheckerLib.sol";
import {RayMathLib} from "./libraries/RayMathLib.sol";

import {IAngstromComposable} from "./interfaces/IAngstromComposable.sol";
import {IUnlockCallback} from "v4-core/src/interfaces/callback/IUnlockCallback.sol";
import {IPoolManager, IUniV4} from "./interfaces/IUniV4.sol";

/// @author philogy <https://github.com/philogy>
contract Angstrom is ERC712, Accounter, UnorderedNonces, PoolRewardsManager, NodeManager, IUnlockCallback {
    using RayMathLib for uint256;
    using IUniV4 for IPoolManager;
    using SafeCastLib for uint256;
    using FixedPointMathLib for uint256;

    error InvalidPoolKey();

    error AssetsOutOfOrder();
    error OnlyOncePerBlock();

    error LimitViolated();
    error Expired();
    error InvalidHookReturn();
    error OrderAlreadyExecuted();

    error FillingTooLittle();
    error FillingTooMuch();
    error InvalidSignature();
    error Unresolved();

    // persistent storage
    uint256 public lastBlockUpdated;
    uint256 public halfSpreadRay;

    // transient storage
    mapping(bytes32 => tuint256) internal alreadyExecuted;

    constructor(address uniV4PoolManager, address governance) UniConsumer(uniV4PoolManager) NodeManager(governance) {
        _checkHookPermissions(Hooks.BEFORE_SWAP_FLAG | Hooks.BEFORE_INITIALIZE_FLAG);
    }

    function beforeInitialize(address, PoolKey calldata poolKey, uint160) external onlyUniV4 returns (bytes4) {
        if (poolKey.tickSpacing != TICK_SPACING || poolKey.fee != SET_POOL_FEE) revert InvalidPoolKey();
        return this.beforeInitialize.selector;
    }

    function execute(bytes calldata data) external onlyNode {
        UNI_V4.unlock(data);
    }

    function unlockCallback(bytes calldata data) external override onlyUniV4 returns (bytes memory) {
        (
            Asset[] calldata assets,
            Price[] calldata initialPrices,
            TopOfBlockOrderEnvelope[] calldata topOfBlockOrders,
            PoolSwap[] calldata swaps,
            GenericOrder[] calldata orders,
            PoolRewardsUpdate[] calldata poolRewardsUpdates
        ) = DecoderLib.unpack(data);

        Globals memory g = _validateAndInitGlobals(assets, initialPrices);

        _borrowAssets(assets);
        _execPoolSwaps(g, swaps);
        _validateAndExecuteToB(g, topOfBlockOrders);
        _rewardPools(g, poolRewardsUpdates);
        _validateAndExecuteOrders(g, orders);
        _saveAndSettle(assets);

        return new bytes(0);
    }

    function _validateAndInitGlobals(Asset[] calldata assets, Price[] calldata initialPrices)
        internal
        returns (Globals memory)
    {
        // Global bundle lock (prevents reentrancy & replaying flash orders).
        if (lastBlockUpdated == block.number) revert OnlyOncePerBlock();
        lastBlockUpdated = block.number;

        // Validate asset list.
        address lastAsset = assets[0].addr;
        for (uint256 i = 1; i < assets.length; i++) {
            address nextAsset = assets[i].addr;
            if (nextAsset <= lastAsset) revert AssetsOutOfOrder();
            lastAsset = nextAsset;
        }

        // Initialize and validate price graph.
        PriceGraph prices = PriceGraphLib.init(assets.length);
        for (uint256 i = 0; i < initialPrices.length; i++) {
            Price calldata init = initialPrices[i];
            prices.set(init.outIndex, init.inIndex, init.price);
        }

        return Globals({prices: prices, assets: assets});
    }

    function _validateAndExecuteToB(Globals memory g, TopOfBlockOrderEnvelope[] calldata orders) internal {
        for (uint256 i = 0; i < orders.length; i++) {
            TopOfBlockOrderEnvelope calldata order = orders[i];

            address assetIn = g.get(order.assetInIndex);
            address assetOut = g.get(order.assetOutIndex);

            // The `.hash` method validates the `block.number` for flash orders.
            bytes32 orderHash = order.hash(assetIn, assetOut);

            tuint256 storage executed = alreadyExecuted[orderHash];
            if (executed.get() != 0) revert OrderAlreadyExecuted();
            executed.set(1);

            if (!SignatureCheckerLib.isValidSignatureNow(order.from, _hashTypedData(orderHash), order.signature)) {
                revert InvalidSignature();
            }

            if (order.hook != address(0)) {
                if (
                    IAngstromComposable(order.hook).compose(order.from, order.hookPayload)
                        != ~uint32(IAngstromComposable.compose.selector)
                ) revert InvalidHookReturn();
            }

            _accountIn(order.from, assetIn, order.amountIn);
            _accountOut(order.from, assetOut, order.amountOut);
        }
    }

    function _validateAndExecuteOrders(Globals memory g, GenericOrder[] calldata orders) internal {
        for (uint256 i = 0; i < orders.length; i++) {
            GenericOrder calldata order = orders[i];
            uint256 price = g.prices.get(order.assetOutIndex, order.assetInIndex);
            if (price < order.minPrice) revert LimitViolated();

            address assetIn = g.get(order.assetInIndex);
            address assetOut = g.get(order.assetOutIndex);
            // The `.hash` method validates the `block.number` for flash orders.
            bytes32 orderHash = order.hash(assetIn, assetOut);

            if (!SignatureCheckerLib.isValidSignatureNow(order.from, _hashTypedData(orderHash), order.signature)) {
                revert InvalidSignature();
            }

            if (order.otype == OrderType.Standing) {
                if (block.timestamp > order.deadline) revert Expired();
                _useNonce(order.from, order.nonce);
            } else {
                tuint256 storage executed = alreadyExecuted[orderHash];
                if (executed.get() != 0) revert OrderAlreadyExecuted();
                executed.set(1);
            }

            if (order.hook != address(0)) {
                if (
                    IAngstromComposable(order.hook).compose(order.from, order.hookPayload)
                        != ~uint32(IAngstromComposable.compose.selector)
                ) revert InvalidHookReturn();
            }

            (uint256 amountIn, uint256 amountOut) = _getAmounts(order, price);
            _accountIn(order.from, assetIn, amountIn);
            _accountOut(order.from, assetOut, amountOut);
        }
    }

    function _getAmounts(GenericOrder calldata order, uint256 price)
        internal
        view
        returns (uint256 amountIn, uint256 amountOut)
    {
        uint256 feeRay = halfSpreadRay;
        if (order.mode == OrderMode.ExactIn) {
            amountIn = order.amountSpecified;
            amountOut = amountIn.divRay(price);
            amountOut -= amountOut.mulRay(feeRay);
        } else if (order.mode == OrderMode.ExactOut) {
            amountOut = order.amountSpecified;
            amountIn = amountOut.mulRay(price);
            amountIn += amountIn.mulRay(feeRay);
        } else if (order.mode == OrderMode.Partial) {
            amountIn = order.amountFilled;
            if (amountIn < order.minAmountIn) revert FillingTooLittle();
            if (amountIn > order.amountSpecified) revert FillingTooMuch();
            amountOut = amountIn.divRay(price);
            amountOut -= amountOut.mulRay(feeRay);
        } else {
            assert(false);
        }
    }
}
