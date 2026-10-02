import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
    getWorldFavorites: vi.fn(),
    getAvatarFavorites: vi.fn(),
    getFriendFavorites: vi.fn(),
    getExplicitLocalFavoriteGroups: vi.fn()
}));

vi.mock('@/repositories/favoritePersistenceRepository', () => ({
    default: {
        getWorldFavorites: mocks.getWorldFavorites,
        getAvatarFavorites: mocks.getAvatarFavorites,
        getFriendFavorites: mocks.getFriendFavorites,
        getExplicitLocalFavoriteGroups: mocks.getExplicitLocalFavoriteGroups
    }
}));

describe('favoriteLocalRefreshService', () => {
    beforeEach(async () => {
        vi.clearAllMocks();
        const { useFavoriteStore } = await import('@/state/favoriteStore');
        useFavoriteStore.getState().resetFavorites();

        mocks.getWorldFavorites.mockResolvedValue([]);
        mocks.getAvatarFavorites.mockResolvedValue([]);
        mocks.getFriendFavorites.mockResolvedValue([]);
        mocks.getExplicitLocalFavoriteGroups.mockResolvedValue([]);
    });

    it('leaves local world favorites out of the frontend store refresh path', async () => {
        const { refreshLocalFavoritesForKinds } =
            await import('./favoriteLocalRefreshService');

        await refreshLocalFavoritesForKinds(['world']);

        expect(mocks.getWorldFavorites).not.toHaveBeenCalled();
        expect(mocks.getAvatarFavorites).not.toHaveBeenCalled();
        expect(mocks.getFriendFavorites).not.toHaveBeenCalled();
    });

    it('deduplicates repeated kinds and refreshes each requested kind once', async () => {
        const { refreshLocalFavoritesForKinds } =
            await import('./favoriteLocalRefreshService');

        await refreshLocalFavoritesForKinds(['avatar', 'avatar', 'friend']);

        expect(mocks.getAvatarFavorites).toHaveBeenCalledTimes(1);
        expect(mocks.getFriendFavorites).toHaveBeenCalledTimes(1);
        expect(mocks.getWorldFavorites).not.toHaveBeenCalled();
    });

    it('keeps the newest result when same-kind refreshes finish out of order', async () => {
        let resolveFirst: (rows: unknown[]) => void = () => undefined;
        mocks.getAvatarFavorites
            .mockImplementationOnce(
                () =>
                    new Promise<unknown[]>((resolve) => {
                        resolveFirst = resolve;
                    })
            )
            .mockResolvedValueOnce([
                {
                    created_at: '2026-01-02',
                    avatarId: 'avtr_new',
                    groupName: 'New'
                }
            ]);
        mocks.getExplicitLocalFavoriteGroups
            .mockResolvedValueOnce(['Old'])
            .mockResolvedValueOnce(['New']);
        const { useFavoriteStore } = await import('@/state/favoriteStore');
        const { refreshLocalFavoritesForKinds } =
            await import('./favoriteLocalRefreshService');

        const first = refreshLocalFavoritesForKinds(['avatar']);
        await refreshLocalFavoritesForKinds(['avatar']);
        resolveFirst([
            {
                created_at: '2026-01-01',
                avatarId: 'avtr_old',
                groupName: 'Old'
            }
        ]);
        await first;

        expect(useFavoriteStore.getState().localAvatarFavorites).toEqual({
            New: ['avtr_new']
        });
    });

    it('drops a completed read after the favorite owner changes', async () => {
        let resolveRows: (rows: unknown[]) => void = () => undefined;
        mocks.getFriendFavorites.mockImplementationOnce(
            () =>
                new Promise<unknown[]>((resolve) => {
                    resolveRows = resolve;
                })
        );
        mocks.getExplicitLocalFavoriteGroups.mockResolvedValue(['Friends']);
        const { useFavoriteStore } = await import('@/state/favoriteStore');
        const { refreshLocalFavoritesForKinds } =
            await import('./favoriteLocalRefreshService');
        useFavoriteStore.getState().setFavoritesLoading('usr_old');

        const refresh = refreshLocalFavoritesForKinds(['friend']);
        useFavoriteStore.getState().setFavoritesLoading('usr_new');
        resolveRows([
            {
                created_at: '2026-01-01',
                userId: 'usr_friend',
                groupName: 'Friends'
            }
        ]);
        await refresh;

        const state = useFavoriteStore.getState();
        expect(state.currentUserId).toBe('usr_new');
        expect(state.localFriendFavorites).toEqual({});
    });
});
