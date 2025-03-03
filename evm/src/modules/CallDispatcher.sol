// Copyright (C) Polytope Labs Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
pragma solidity ^0.8.17;

import {ICallDispatcher} from "../interfaces/ICallDispatcher.sol";

struct CallDispatcherParams {
    // contract to call
    address target;
    // target contract calldata
    bytes data;
}

/**
 * @title The CallDispatcher
 * @author Polytope Labs (hello@polytope.technology)
 *
 * @notice This contract is used to dispatch calls to other contracts.
 */
contract CallDispatcher is ICallDispatcher {
    /**
     *  @dev reverts if the target is not a contract or if any of the calls reverts.
     */
    function dispatch(bytes memory encoded) external {
        CallDispatcherParams[] memory calls = abi.decode(encoded, (CallDispatcherParams[]));
        uint256 callsLen = calls.length;
        for (uint256 i = 0; i < callsLen; ++i) {
            CallDispatcherParams memory call = calls[i];
            uint32 size;
            address target = call.target;
            assembly {
                size := extcodesize(target)
            }

            if (size > 0) {
                (bool success, bytes memory result) = target.call(call.data);
                if (!success) revert(string(result));
            }
        }
    }
}
