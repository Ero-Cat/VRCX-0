// @vitest-environment jsdom

import { act, renderHook } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';

import { useUserFactsStore } from '@/state/userFactsStore';

import { useKnownUserOptions } from './useKnownUserOptions';

const endpoint = 'https://api.vrchat.cloud';

describe('useKnownUserOptions', () => {
    beforeEach(() => {
        useUserFactsStore.getState().resetUserFacts();
    });

    it('finds users whose names sort past the option limit when searched', () => {
        act(() => {
            useUserFactsStore.getState().replaceUserFacts([
                ...Array.from({ length: 600 }, (_, index) => ({
                    id: `usr_a${index}`,
                    endpoint,
                    displayName: `Alice ${String(index).padStart(3, '0')}`
                })),
                { id: 'usr_zoe', endpoint, displayName: 'Zoe' }
            ]);
        });

        const { result, rerender } = renderHook(
            ({ query }) =>
                useKnownUserOptions({ enabled: true, endpoint, query }),
            { initialProps: { query: '' } }
        );

        expect(result.current).toHaveLength(500);
        expect(result.current.some((user) => user.id === 'usr_zoe')).toBe(
            false
        );

        rerender({ query: 'zo' });

        expect(result.current.map((user) => user.id)).toEqual(['usr_zoe']);
    });
});
