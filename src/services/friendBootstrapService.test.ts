import { beforeEach, describe, expect, it, vi } from 'vitest';

const serviceMocks = vi.hoisted(() => ({
    socialFriendRosterBaselineGet: vi.fn(),
    signalFriendLogChanged: vi.fn()
}));

vi.mock('@/platform/tauri/bindings', () => ({
    commands: {
        appSocialFriendRosterBaselineGet:
            serviceMocks.socialFriendRosterBaselineGet
    }
}));

vi.mock('./friendLogMutationService', () => ({
    signalFriendLogChanged: serviceMocks.signalFriendLogChanged
}));

describe('friendBootstrapService baseline reconciliation', () => {
    beforeEach(async () => {
        vi.clearAllMocks();

        const { useFriendRosterStore } =
            await import('@/state/friendRosterStore');
        const { useRuntimeStore } = await import('@/state/runtimeStore');
        const { useSessionStore } = await import('@/state/sessionStore');

        useFriendRosterStore.getState().resetRoster();
        useRuntimeStore.getState().resetRuntimeState();
        useSessionStore.getState().resetSessionState();
        useRuntimeStore.getState().setAuthBootstrap({
            currentUserId: 'usr_self',
            currentUserEndpoint: 'https://api.example.test',
            currentUserWebsocket: 'wss://ws.example.test',
            currentUserSnapshot: {
                id: 'usr_self'
            }
        });
        useSessionStore.getState().setSessionState({
            isLoggedIn: true,
            isFriendsLoaded: true,
            sessionPhase: 'ready'
        });
        serviceMocks.socialFriendRosterBaselineGet.mockResolvedValue({
            stale: false,
            count: 0,
            detail: 'complete',
            snapshot: {
                currentUserId: 'usr_self',
                generation: 1,
                presenceById: {},
                friendsById: {}
            }
        });
    });

    it('marks friends loaded after the fast roster snapshot', async () => {
        const { useFriendRosterStore } =
            await import('@/state/friendRosterStore');
        const { useSessionStore } = await import('@/state/sessionStore');
        const { bootstrapFriendRoster } =
            await import('./friendBootstrapService');
        serviceMocks.socialFriendRosterBaselineGet.mockResolvedValue({
            stale: false,
            count: 2,
            detail: 'fast roster',
            snapshot: {
                currentUserId: 'usr_self',
                generation: 1,
                presenceById: {},
                friendsById: {
                    usr_online: {
                        id: 'usr_online',
                        displayName: 'Online Fast',
                        state: 'online',
                        platform: 'standalonewindows',
                        location: 'wrld_live:123'
                    },
                    usr_traveling: {
                        id: 'usr_traveling',
                        displayName: 'Traveling Fast',
                        state: 'online',
                        platform: 'standalonewindows',
                        location: 'traveling'
                    }
                }
            }
        });

        await bootstrapFriendRoster({
            userId: 'usr_self',
            endpoint: 'https://api.example.test',
            currentUserSnapshot: {
                id: 'usr_self',
                friends: ['usr_online', 'usr_traveling', 'usr_missing'],
                offlineFriends: ['usr_missing'],
                activeFriends: [],
                onlineFriends: ['usr_online', 'usr_traveling']
            }
        });

        expect(useSessionStore.getState().isFriendsLoaded).toBe(true);
        expect(useFriendRosterStore.getState()).toMatchObject({
            loadStatus: 'ready',
            detail: 'fast roster',
            friendsById: {
                usr_online: {
                    displayName: 'Online Fast',
                    location: 'wrld_live:123'
                },
                usr_traveling: {
                    displayName: 'Traveling Fast',
                    location: 'traveling'
                }
            }
        });
    });

    it('signals the friend log store when the baseline reports a friend-log change', async () => {
        const { bootstrapFriendRoster } =
            await import('./friendBootstrapService');
        serviceMocks.socialFriendRosterBaselineGet.mockResolvedValue({
            stale: false,
            count: 1,
            detail: 'fast roster',
            friendLogChanged: true,
            snapshot: {
                currentUserId: 'usr_self',
                generation: 1,
                presenceById: {},
                friendsById: {
                    usr_online: {
                        id: 'usr_online',
                        displayName: 'Online Fast',
                        state: 'online'
                    }
                }
            }
        });

        await bootstrapFriendRoster({
            userId: 'usr_self',
            endpoint: 'https://api.example.test',
            currentUserSnapshot: {
                id: 'usr_self',
                friends: ['usr_online'],
                offlineFriends: [],
                activeFriends: [],
                onlineFriends: ['usr_online']
            }
        });

        expect(serviceMocks.signalFriendLogChanged).toHaveBeenCalledOnce();
    });

    it('does not signal the friend log store when the baseline reports no friend-log change', async () => {
        const { bootstrapFriendRoster } =
            await import('./friendBootstrapService');
        serviceMocks.socialFriendRosterBaselineGet.mockResolvedValue({
            stale: false,
            count: 1,
            detail: 'fast roster',
            friendLogChanged: false,
            snapshot: {
                currentUserId: 'usr_self',
                generation: 1,
                presenceById: {},
                friendsById: {
                    usr_online: {
                        id: 'usr_online',
                        displayName: 'Online Fast',
                        state: 'online'
                    }
                }
            }
        });

        await bootstrapFriendRoster({
            userId: 'usr_self',
            endpoint: 'https://api.example.test',
            currentUserSnapshot: {
                id: 'usr_self',
                friends: ['usr_online'],
                offlineFriends: [],
                activeFriends: [],
                onlineFriends: ['usr_online']
            }
        });

        expect(serviceMocks.signalFriendLogChanged).not.toHaveBeenCalled();
    });

    it('keeps realtime patches authoritative when refreshing a loaded roster', async () => {
        const { useFriendRosterStore } =
            await import('@/state/friendRosterStore');
        const { bootstrapFriendRoster } =
            await import('./friendBootstrapService');
        useFriendRosterStore.getState().setRosterSnapshot({
            currentUserId: 'usr_self',
            friendsById: {
                usr_live: {
                    id: 'usr_live',
                    displayName: 'Live Friend',
                    state: 'online',
                    location: 'wrld_new:456'
                }
            }
        });
        serviceMocks.socialFriendRosterBaselineGet.mockResolvedValue({
            stale: false,
            count: 1,
            detail: 'refreshed',
            snapshot: {
                currentUserId: 'usr_self',
                generation: 1,
                presenceById: {},
                friendsById: {
                    usr_stale: {
                        id: 'usr_stale',
                        displayName: 'Stale Friend',
                        state: 'offline'
                    }
                }
            }
        });

        await bootstrapFriendRoster({
            userId: 'usr_self',
            endpoint: 'https://api.example.test',
            websocket: 'wss://ws.example.test',
            currentUserSnapshot: { id: 'usr_self' },
            preserveLoadedState: true
        });

        expect(useFriendRosterStore.getState()).toMatchObject({
            loadStatus: 'ready',
            detail: 'refreshed',
            friendsById: {
                usr_live: {
                    displayName: 'Live Friend',
                    state: 'online',
                    location: 'wrld_new:456'
                }
            }
        });
        expect(
            useFriendRosterStore.getState().friendsById.usr_stale
        ).toBeUndefined();
    });

    it('skips a superseded refresh without erroring when preserving loaded state', async () => {
        const { useFriendRosterStore } =
            await import('@/state/friendRosterStore');
        const { bootstrapFriendRoster } =
            await import('./friendBootstrapService');
        useFriendRosterStore.getState().setRosterSnapshot({
            currentUserId: 'usr_self',
            friendsById: {
                usr_live: {
                    id: 'usr_live',
                    displayName: 'Live Friend',
                    state: 'online'
                }
            }
        });
        serviceMocks.socialFriendRosterBaselineGet.mockResolvedValue({
            stale: true,
            count: 0,
            detail: 'Superseded friend roster baseline.',
            snapshot: null
        });

        const result = await bootstrapFriendRoster({
            userId: 'usr_self',
            endpoint: 'https://api.example.test',
            websocket: 'wss://ws.example.test',
            currentUserSnapshot: { id: 'usr_self' },
            preserveLoadedState: true
        });

        expect(result.stale).toBe(true);
        expect(useFriendRosterStore.getState()).toMatchObject({
            loadStatus: 'ready',
            detail: 'Superseded friend roster baseline.',
            friendsById: {
                usr_live: { displayName: 'Live Friend' }
            }
        });
    });

    it('reports a failed Rust baseline without marking friends loaded', async () => {
        const { useFriendRosterStore } =
            await import('@/state/friendRosterStore');
        const { useSessionStore } = await import('@/state/sessionStore');
        const { bootstrapFriendRoster } =
            await import('./friendBootstrapService');
        serviceMocks.socialFriendRosterBaselineGet.mockRejectedValue(
            new Error('baseline failed')
        );

        await expect(
            bootstrapFriendRoster({
                userId: 'usr_self',
                endpoint: 'https://api.example.test',
                currentUserSnapshot: { id: 'usr_self' }
            })
        ).rejects.toThrow('baseline failed');

        expect(useFriendRosterStore.getState()).toMatchObject({
            loadStatus: 'error',
            detail: 'baseline failed'
        });
        expect(useFriendRosterStore.getState().friendsById).toEqual({});
        expect(useSessionStore.getState().isFriendsLoaded).toBe(false);
    });

    it('reports a stale Rust baseline without marking friends loaded', async () => {
        const { useFriendRosterStore } =
            await import('@/state/friendRosterStore');
        const { useSessionStore } = await import('@/state/sessionStore');
        const { bootstrapFriendRoster } =
            await import('./friendBootstrapService');
        serviceMocks.socialFriendRosterBaselineGet.mockResolvedValue({
            stale: true,
            count: 0,
            detail: 'stale baseline'
        });

        await expect(
            bootstrapFriendRoster({
                userId: 'usr_self',
                endpoint: 'https://api.example.test',
                currentUserSnapshot: { id: 'usr_self' }
            })
        ).rejects.toThrow('Friend roster baseline was stale for usr_self.');

        expect(useFriendRosterStore.getState()).toMatchObject({
            loadStatus: 'error',
            detail: 'Friend roster baseline was stale for usr_self.'
        });
        expect(useFriendRosterStore.getState().friendsById).toEqual({});
        expect(useSessionStore.getState().isFriendsLoaded).toBe(false);
    });
});
