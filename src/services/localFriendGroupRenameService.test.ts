import { beforeEach, describe, expect, it, vi } from 'vitest';

const stored = vi.hoisted(() => new Map<string, string>());
const configRepository = vi.hoisted(() => ({
    getString: vi.fn(async (key: string, fallback: string) =>
        stored.has(key) ? stored.get(key) : fallback
    ),
    setString: vi.fn(async (key: string, value: string) => {
        stored.set(key, value);
    })
}));
const setLocalFavoriteFriendsGroupsPreference = vi.hoisted(() => vi.fn());
const publishPreferenceChanged = vi.hoisted(() => vi.fn());

vi.mock('@/repositories/configRepository', () => ({
    default: configRepository
}));
vi.mock('@/services/preferencesService', () => ({
    setLocalFavoriteFriendsGroupsPreference
}));
vi.mock('@/shared/events/preferenceEvents', () => ({
    publishPreferenceChanged
}));

import { usePreferencesStore } from '@/state/preferencesStore';
import { useSidebarTabStore } from '@/state/sidebarTabStore';

import { renameLocalFriendGroupReferences } from './localFriendGroupRenameService';

const storedJson = (key: string) => JSON.parse(stored.get(key) ?? 'null');

describe('renameLocalFriendGroupReferences', () => {
    beforeEach(() => {
        stored.clear();
        vi.clearAllMocks();
    });

    it('rewrites every saved selection of the renamed local friend group', async () => {
        stored.set(
            'sidebarFavoriteGroups',
            JSON.stringify(['group_0', 'local:Close'])
        );
        stored.set(
            'sidebarFavoriteGroupOrder',
            JSON.stringify(['local:Close', 'local:Closer'])
        );
        stored.set('autoAcceptInviteGroups', JSON.stringify(['local:Close']));
        stored.set(
            'feedColumnsConfig',
            JSON.stringify([
                {
                    id: 'column',
                    friendScope: {
                        kind: 'favorites',
                        groupKeys: ['local:Close'],
                        excludedFavoriteGroupKeys: 'all'
                    }
                }
            ])
        );
        stored.set(
            'sidebarTabLayout',
            JSON.stringify([
                {
                    id: 'close-friends',
                    type: 'favoriteCollection',
                    name: 'Close',
                    icon: 'lucide:UserStar',
                    visible: true,
                    sourceGroupKeys: ['local:Close', 'group_1']
                }
            ])
        );
        usePreferencesStore.setState({
            localFavoriteFriendsGroups: ['local:Close', 'group_1']
        });

        await renameLocalFriendGroupReferences('Close', 'Inner');

        expect(storedJson('sidebarFavoriteGroups')).toEqual([
            'group_0',
            'local:Inner'
        ]);
        expect(storedJson('sidebarFavoriteGroupOrder')).toEqual([
            'local:Inner',
            'local:Closer'
        ]);
        expect(storedJson('autoAcceptInviteGroups')).toEqual(['local:Inner']);
        expect(
            storedJson('feedColumnsConfig')[0].friendScope.groupKeys
        ).toEqual(['local:Inner']);
        const collectionTab = (layout: unknown) =>
            Array.isArray(layout)
                ? layout.find((item) => item.id === 'close-friends')
                : undefined;
        expect(
            collectionTab(storedJson('sidebarTabLayout')).sourceGroupKeys
        ).toEqual(['local:Inner', 'group_1']);
        expect(
            collectionTab(useSidebarTabStore.getState().tabLayout)
        ).toMatchObject({ sourceGroupKeys: ['local:Inner', 'group_1'] });
        expect(setLocalFavoriteFriendsGroupsPreference).toHaveBeenCalledWith([
            'local:Inner',
            'group_1'
        ]);
        expect(publishPreferenceChanged).toHaveBeenCalledWith(
            'sidebarFavoriteGroups',
            ['group_0', 'local:Inner']
        );
        expect(publishPreferenceChanged).toHaveBeenCalledWith(
            'sidebarFavoriteGroupOrder',
            ['local:Inner', 'local:Closer']
        );
    });

    it('leaves selections without the renamed group untouched', async () => {
        stored.set('sidebarFavoriteGroups', JSON.stringify(['local:Closer']));
        usePreferencesStore.setState({ localFavoriteFriendsGroups: [] });

        await renameLocalFriendGroupReferences('Close', 'Inner');

        expect(configRepository.setString).not.toHaveBeenCalledWith(
            'sidebarFavoriteGroups',
            expect.anything()
        );
        expect(setLocalFavoriteFriendsGroupsPreference).not.toHaveBeenCalled();
    });
});
