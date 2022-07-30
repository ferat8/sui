// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { isSuiMoveObject } from '@mysten/sui.js';
import { createSelector } from '@reduxjs/toolkit';

import { Delegation } from './Delegation';
import { ownedObjects } from '_redux/slices/account';
import { suiObjectsAdapterSelectors } from '_redux/slices/sui-objects';
import { SUI_SYSTEM_STATE_OBJECT_ID } from '_redux/slices/sui-objects/Coin';

import type { DelegationSuiObject } from './Delegation';
import type { RootState } from '_redux/RootReducer';

export const delegationsSelector = createSelector(
    ownedObjects,
    (objects) =>
        objects.filter((obj) =>
            Delegation.isDelegationSuiObject(obj)
        ) as DelegationSuiObject[]
);

export const activeDelegationsSelector = createSelector(
    delegationsSelector,
    (delegations) => delegations.filter((obj) => new Delegation(obj).isActive())
);

export const activeDelegationIDsSelector = createSelector(
    activeDelegationsSelector,
    (delegations) => delegations.map(({ reference: { objectId } }) => objectId)
);

export const totalActiveStakedSelector = createSelector(
    activeDelegationsSelector,
    (activeDelegations) =>
        activeDelegations.reduce((total, obj) => {
            total += BigInt(new Delegation(obj).activeDelegation);
            return total;
        }, BigInt(0))
);

export const epochSelector = (state: RootState) => {
    const { data } =
        suiObjectsAdapterSelectors.selectById(
            state,
            SUI_SYSTEM_STATE_OBJECT_ID
        ) || {};
    console.log(data, isSuiMoveObject(data));
    return isSuiMoveObject(data) ? (data.fields.epoch as number) : null;
};
