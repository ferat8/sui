// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import cl from 'classnames';
import { memo, useMemo } from 'react';
import { useIntl } from 'react-intl';

import CoinBalance from '_app/shared/coin-balance';
import { Delegation } from '_app/staking/Delegation';
import { epochSelector } from '_app/staking/selectors';
import Icon, { SuiIcons } from '_components/icon';
import { useAppSelector } from '_hooks';
import { suiObjectsAdapterSelectors } from '_redux/slices/sui-objects/index';
import { percentageFormatOptions } from '_shared/formatting';

import type { RootState } from '_redux/RootReducer';

import st from './DelegationCard.module.scss';

export type DelegationCardProps = {
    className?: string;
    id: string;
};

function DelegationCard({ className, id }: DelegationCardProps) {
    const delegationSelector = useMemo(
        () => (state: RootState) => {
            const suiObj = suiObjectsAdapterSelectors.selectById(state, id);
            if (suiObj && Delegation.isDelegationSuiObject(suiObj)) {
                return new Delegation(suiObj);
            }
            return undefined;
        },
        [id]
    );
    const delegation = useAppSelector(delegationSelector);
    const epoch = useAppSelector(epochSelector);
    console.log(delegation, epoch);
    const intl = useIntl();
    return (
        <div className={cl(st.container, className)}>
            <div className={st.iconRow}>
                <Icon icon="columns-gap" />
            </div>
            <div className={st.validator}>-</div>
            <div className={st.apy}>
                {intl.formatNumber(1 / 100, percentageFormatOptions)} APY
            </div>
            {/* <div className={st.balance}>
                <CoinBalance
                    balance={balance}
                    symbol={symbol}
                    className={st.balance}
                />
            </div>
            {rewards ? (
                <Icon icon="circle-fill" className={st.rewards} />
            ) : null} */}
            <Icon icon={SuiIcons.ArrowRight} className={st.arrow} />
        </div>
    );
}

export default memo(DelegationCard);
