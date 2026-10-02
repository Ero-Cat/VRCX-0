import { create } from 'zustand';

import type { SyncStatusSnapshot } from '@/platform/tauri/bindings';

type SyncStatusState = {
    snapshot: SyncStatusSnapshot | null;
    apply: (snapshot: SyncStatusSnapshot) => void;
};

/**
 * Live sync status pushed by the engine after every completed cycle
 * ("syncStatusChanged" runtime event); polling backfills between cycles.
 */
export const useSyncStatusStore = create<SyncStatusState>((set) => ({
    snapshot: null,
    apply: (snapshot) => set({ snapshot })
}));
