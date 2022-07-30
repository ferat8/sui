// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { isSuiMoveObject } from '@mysten/sui.js';

import type {
    SuiAddress,
    SuiData,
    SuiMoveObject,
    SuiObject,
} from '@mysten/sui.js';

export type DelegationData = SuiMoveObject &
    Pick<SuiData, 'dataType'> & {
        type: '0x2::delegation::Delegation';
        fields: {
            active_delegation: number;
            delegate_amount: number;
            next_reward_unclaimed_epoch: number;
            validator_address: SuiAddress;
            info: {
                id: string;
                version: number;
            };
            coin_locked_until_epoch: {
                fields: { vec: string };
                type: string;
            };
            ending_epoch: {
                fields: { vec: string };
                type: string;
            };
        };
    };

export type DelegationSuiObject = Omit<SuiObject, 'data'> & {
    data: DelegationData;
};

export class Delegation {
    public static readonly SUI_OBJECT_TYPE = '0x2::delegation::Delegation';
    private suiObject: DelegationSuiObject;

    public static isDelegationSuiObject(
        obj: SuiObject
    ): obj is DelegationSuiObject {
        return (
            isSuiMoveObject(obj.data) &&
            obj.data.type === Delegation.SUI_OBJECT_TYPE
        );
    }

    constructor(obj: DelegationSuiObject) {
        this.suiObject = obj;
    }

    public get nextRewardUnclaimedEpoch() {
        return this.suiObject.data.fields.next_reward_unclaimed_epoch;
    }

    public get activeDelegation() {
        // TODO: this probably is wrong - check how option works
        return this.suiObject.data.fields.active_delegation;
    }

    public get delegateAmount() {
        return this.suiObject.data.fields.delegate_amount;
    }

    public get endingEpoch() {
        // TODO: this probably is wrong - check how option works
        return this.suiObject.data.fields.ending_epoch.fields.vec;
    }

    public get validatorAddress() {
        return this.suiObject.data.fields.validator_address;
    }

    public isActive() {
        return this.activeDelegation > 0 && !this.endingEpoch;
    }

    public hasUnclaimedRewards(epoch: number) {
        // TODO
        return this.nextRewardUnclaimedEpoch <= epoch;
    }
}
