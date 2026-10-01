import { commands } from '@/platform/tauri/bindings';
import type {
    SyncBootstrapProgress,
    SyncConnectionFields,
    SyncConnectionInput,
    SyncConnectionTestResult,
    SyncStatusSnapshot
} from '@/platform/tauri/bindings';

export interface SyncConfigureInput {
    enabled?: boolean;
    connection?: SyncConnectionInput;
    intervalSec?: number;
}

export async function fetchSyncStatus(): Promise<SyncStatusSnapshot> {
    return commands.syncStatus();
}

export async function fetchSyncConnection(): Promise<SyncConnectionFields> {
    return commands.syncGetConnection();
}

export async function fetchSyncBootstrapProgress(): Promise<SyncBootstrapProgress> {
    return commands.syncBootstrapProgress();
}

export async function configureSync(
    input: SyncConfigureInput
): Promise<SyncStatusSnapshot> {
    return commands.syncConfigure(
        input.enabled ?? null,
        input.connection ?? null,
        input.intervalSec ?? null
    );
}

export async function testSyncConnection(
    connection: SyncConnectionInput
): Promise<SyncConnectionTestResult> {
    return commands.syncTestConnection(connection);
}

export async function triggerSyncNow(): Promise<SyncStatusSnapshot> {
    return commands.syncTriggerNow();
}
