// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import type { SuiEventType } from '../../components/events/eventTypes';
import type {
    CertifiedTransaction,
    ExecutionStatusType,
    SuiObjectRef,
} from '@mysten/sui.js';

export type DataType = CertifiedTransaction & {
    loadState: string;
    txId: string;
    status: ExecutionStatusType;
    gasFee: number;
    txError: string;
    mutated: SuiObjectRef[];
    created: SuiObjectRef[];
    events: SuiEventType[];
    timestamp_ms: number | null;
};

export type Category =
    | 'objects'
    | 'transactions'
    | 'addresses'
    | 'ethAddress'
    | 'unknown';
