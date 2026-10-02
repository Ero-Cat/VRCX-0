import {
    commands,
    type HostSessionProjection
} from '@/platform/tauri/bindings';
import { resetGameLogSessionState } from '@/services/gameLogIngestService';
import { useRuntimeStore } from '@/state/runtimeStore';
import { useSessionStore } from '@/state/sessionStore';

type RuntimeState = ReturnType<typeof useRuntimeStore.getState>;
type GameStatePatch = Parameters<RuntimeState['setGameState']>[0];

async function handleGameStopped() {
    const stoppedAt = new Date().toISOString();
    useRuntimeStore.getState().clearInstanceQueueState();

    resetGameLogSessionState(stoppedAt);

    await commands.appRuntimeDiscordReconcileRequest().catch((error) => {
        console.warn(
            'Discord presence reconcile after game stop failed:',
            error
        );
    });
}

function buildNewGameSessionPatch(startedAt: string): GameStatePatch {
    return {
        currentLocation: '',
        currentWorldId: '',
        currentWorldName: '',
        currentDestination: '',
        currentLocationStartedAt: null,
        currentLocationPlayerIds: [],
        currentLocationPlayers: [],
        lastGameStartedAt: startedAt
    };
}

function buildStoppedGameSessionPatch(stoppedAt: string): GameStatePatch {
    return {
        currentLocation: '',
        currentWorldId: '',
        currentWorldName: '',
        currentDestination: '',
        currentLocationStartedAt: null,
        currentLocationPlayerIds: [],
        currentLocationPlayers: [],
        lastGameLogAt: stoppedAt,
        lastGameLogType: 'game-stopped'
    };
}

export async function handleGameRunningUpdate(
    projection: HostSessionProjection
) {
    const runtimeStore = useRuntimeStore.getState();
    const previousGameRunning = runtimeStore.gameState.isGameRunning;
    const previousSteamVrRunning = runtimeStore.gameState.isSteamVRRunning;
    const nextGameRunning = projection.isGameRunning;
    const nextSteamVrRunning = projection.isSteamVRRunning;
    const gameRunningChanged = previousGameRunning !== nextGameRunning;
    const steamVrRunningChanged = previousSteamVrRunning !== nextSteamVrRunning;
    const changed = gameRunningChanged || steamVrRunningChanged;
    const payloadChangedAt =
        projection.lastGameStateChangedAt || projection.changedAt;
    const payloadStartedAt = projection.lastGameStartedAt || '';
    const shouldRefreshDiscordPresence =
        gameRunningChanged ||
        (nextGameRunning === true &&
            useSessionStore.getState().sessionPhase === 'ready');
    const now = payloadChangedAt || new Date().toISOString();
    const gameStartedAt =
        gameRunningChanged && nextGameRunning
            ? payloadStartedAt || now
            : payloadStartedAt || runtimeStore.gameState.lastGameStartedAt;
    const newSessionPatch =
        gameRunningChanged && nextGameRunning
            ? buildNewGameSessionPatch(gameStartedAt ?? now)
            : {};
    const stoppedSessionPatch =
        gameRunningChanged && previousGameRunning === true && !nextGameRunning
            ? buildStoppedGameSessionPatch(now)
            : {};

    runtimeStore.setGameState({
        isGameRunning: nextGameRunning,
        isSteamVRRunning: nextSteamVrRunning,
        lastGameStateChangedAt: changed
            ? now
            : runtimeStore.gameState.lastGameStateChangedAt,
        lastGameStartedAt: gameStartedAt,
        ...newSessionPatch,
        ...stoppedSessionPatch
    });

    if (nextGameRunning && gameRunningChanged) {
        useRuntimeStore.getState().resetNowPlayingState();
    }

    if (
        gameRunningChanged &&
        previousGameRunning === true &&
        !nextGameRunning
    ) {
        await handleGameStopped();
        return;
    }

    if (shouldRefreshDiscordPresence) {
        await commands.appRuntimeDiscordReconcileRequest().catch((error) => {
            console.warn(
                'Discord presence reconcile after game state update failed:',
                error
            );
        });
    }
}
