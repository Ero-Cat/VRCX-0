import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
    appRuntimeDiscordReconcileRequest: vi.fn(),
    resetGameLogSessionState: vi.fn()
}));

vi.mock('@/platform/tauri/bindings', () => ({
    commands: {
        appRuntimeDiscordReconcileRequest:
            mocks.appRuntimeDiscordReconcileRequest
    }
}));

vi.mock('@/services/gameLogIngestService', () => ({
    resetGameLogSessionState: mocks.resetGameLogSessionState
}));

import { useRuntimeStore } from '@/state/runtimeStore';
import { useSessionStore } from '@/state/sessionStore';

import { handleGameRunningUpdate } from './gameStateService';

describe('gameStateService lifecycle transitions', () => {
    beforeEach(() => {
        vi.useFakeTimers();
        vi.setSystemTime(new Date('2026-06-08T10:00:00.000Z'));
        vi.clearAllMocks();
        useRuntimeStore.getState().resetRuntimeState();
        useSessionStore.getState().resetSessionState();
        useSessionStore.getState().setSessionState({
            sessionPhase: 'ready',
            isLoggedIn: true
        });
        mocks.appRuntimeDiscordReconcileRequest.mockResolvedValue(1);
    });

    afterEach(() => {
        vi.useRealTimers();
    });

    it('starts a new game session by clearing location mirrors and starting avatar timing', async () => {
        useRuntimeStore.getState().setGameState({
            isGameRunning: false,
            isSteamVRRunning: false,
            currentLocation: 'wrld_old:123',
            currentWorldId: 'wrld_old',
            currentWorldName: 'Old World',
            currentDestination: 'wrld_next:456',
            currentLocationStartedAt: '2026-06-08T09:00:00.000Z',
            currentLocationPlayerIds: ['usr_friend']
        });
        useRuntimeStore.getState().setNowPlayingState({
            url: 'https://video.example/test',
            name: 'Video',
            updatedAt: '2026-06-08T09:59:00.000Z'
        });

        await handleGameRunningUpdate({
            isGameRunning: true,
            isSteamVRRunning: true,
            lastGameStartedAt: '2026-06-08T10:00:00.000Z',
            lastGameStateChangedAt: '2026-06-08T10:00:00.000Z',
            generation: 1,
            gameChanged: true,
            steamvrChanged: true,
            changedAt: '2026-06-08T10:00:00.000Z'
        });

        expect(useRuntimeStore.getState().gameState).toMatchObject({
            isGameRunning: true,
            isSteamVRRunning: true,
            currentLocation: '',
            currentWorldId: '',
            currentWorldName: '',
            currentDestination: '',
            currentLocationStartedAt: null,
            currentLocationPlayerIds: [],
            currentLocationPlayers: [],
            lastGameStartedAt: '2026-06-08T10:00:00.000Z',
            lastGameStateChangedAt: '2026-06-08T10:00:00.000Z'
        });
        expect(useRuntimeStore.getState().nowPlaying).toMatchObject({
            url: '',
            name: ''
        });
        expect(mocks.appRuntimeDiscordReconcileRequest).toHaveBeenCalledTimes(
            1
        );
    });

    it('stops a game session by clearing the local game state', async () => {
        useRuntimeStore.getState().setGameState({
            isGameRunning: true,
            isSteamVRRunning: true,
            currentLocation: 'wrld_old:123',
            currentWorldId: 'wrld_old',
            currentWorldName: 'Old World',
            currentDestination: 'wrld_next:456',
            lastGameStartedAt: '2026-06-08T09:00:00.000Z'
        });
        useRuntimeStore.getState().setInstanceQueueState({
            active: true,
            instanceLocation: 'wrld_old:123',
            position: 2,
            queueSize: 5,
            label: 'Queue'
        });
        await handleGameRunningUpdate({
            isGameRunning: false,
            isSteamVRRunning: false,
            generation: 2,
            gameChanged: true,
            steamvrChanged: true,
            changedAt: '2026-06-08T10:00:00.000Z'
        });

        expect(useRuntimeStore.getState().gameState).toMatchObject({
            isGameRunning: false,
            isSteamVRRunning: false,
            currentLocation: '',
            currentWorldId: '',
            currentWorldName: '',
            currentDestination: '',
            lastGameLogAt: '2026-06-08T10:00:00.000Z',
            lastGameLogType: 'game-stopped'
        });
        expect(useRuntimeStore.getState().instanceQueue.active).toBe(false);
        expect(mocks.resetGameLogSessionState).toHaveBeenCalledWith(
            '2026-06-08T10:00:00.000Z'
        );
        expect(mocks.appRuntimeDiscordReconcileRequest).toHaveBeenCalledTimes(
            1
        );
    });
});
