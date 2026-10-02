// @vitest-environment jsdom

import { act, renderHook } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
    setString: vi.fn()
}));

vi.mock('@/repositories/configRepository', async (importOriginal) => {
    const actual =
        await importOriginal<
            typeof import('@/repositories/configRepository')
        >();
    return {
        default: Object.assign(Object.create(actual.default), {
            setString: mocks.setString
        })
    };
});

import { usePreferencesStore } from '@/state/preferencesStore';

import { useSettingsActions } from './useSettingsActions';

type SettingsActionsDeps = Parameters<typeof useSettingsActions>[0];

function renderSettingsActions() {
    const setPrefs = (
        value: (
            current: ReturnType<typeof usePreferencesStore.getState>
        ) => Record<string, unknown>
    ) => {
        const store = usePreferencesStore.getState();
        store.patchPreferences(value(store));
    };
    const deps = {
        commit: async (
            action: () => Promise<unknown>,
            optimistic?: () => unknown
        ) => {
            optimistic?.();
            await action();
            return true;
        },
        prefs: usePreferencesStore.getState(),
        tableLimitsDraft: { maxTableSize: '1000', searchLimit: '1000' },
        setPrefs
    } as unknown as SettingsActionsDeps;
    return renderHook(() => useSettingsActions(deps));
}

describe('useSettingsActions feed hidden users', () => {
    beforeEach(() => {
        mocks.setString.mockReset();
        mocks.setString.mockResolvedValue(null);
        usePreferencesStore.setState({ feedHiddenUsers: [] });
    });

    it('persists added and removed hidden friends to the config', async () => {
        const { result } = renderSettingsActions();

        await act(() => result.current.addFeedHiddenUser('usr_hidden'));
        expect(mocks.setString).toHaveBeenLastCalledWith(
            'feedHiddenUsers',
            '["usr_hidden"]'
        );
        expect(usePreferencesStore.getState().feedHiddenUsers).toEqual([
            'usr_hidden'
        ]);

        await act(() => result.current.removeFeedHiddenUser('usr_hidden'));
        expect(mocks.setString).toHaveBeenLastCalledWith(
            'feedHiddenUsers',
            '[]'
        );
        expect(usePreferencesStore.getState().feedHiddenUsers).toEqual([]);
    });
});
