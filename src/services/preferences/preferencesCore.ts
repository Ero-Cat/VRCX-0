import { commands } from '@/platform/tauri/bindings';
import {
    normalizePreferenceKey,
    publishPreferenceChanged
} from '@/shared/events/preferenceEvents';
import {
    normalizeTableLimits,
    normalizeTablePageSize,
    normalizeTablePageSizes,
    type PreferencesSnapshot,
    usePreferencesStore
} from '@/state/preferencesStore';
import { normalizeTableDensity, type TableDensity } from '@/state/shellStore';

import { DEFAULT_TABLE_PAGE_SIZE } from './preferencesConstants';
import type {
    PreferenceKey,
    StorePreferenceConfigKey
} from './preferencesTypes';

export function setDocumentLanguage(language: string) {
    document.documentElement.setAttribute('lang', language);
}

export function applyAccessibleStatusClass(enabled: boolean) {
    document.documentElement.classList.toggle(
        'accessible-status-indicators',
        enabled
    );
}

export function applyTableDensityClass(density: TableDensity) {
    const normalized = normalizeTableDensity(density);
    document.documentElement.classList.remove('is-compact-table');
    if (normalized === 'compact') {
        document.documentElement.classList.add('is-compact-table');
    }
}

export function applyDataTableStripedClass(enabled: boolean) {
    document.documentElement.classList.toggle('is-striped-table', enabled);
}

export function applyReducedMotionAndBlurClass(enabled: boolean) {
    document.documentElement.classList.toggle('reduce-effects', enabled);
}

export function patchPreferences(patch: Partial<PreferencesSnapshot>) {
    usePreferencesStore.getState().patchPreferences(patch);
}

function normalizeStorePreferenceKey(
    key: StorePreferenceConfigKey
): PreferenceKey {
    return normalizePreferenceKey(key) as PreferenceKey;
}

export function patchPreferenceValue(
    key: StorePreferenceConfigKey,
    value: PreferencesSnapshot[PreferenceKey]
) {
    usePreferencesStore
        .getState()
        .setPreferenceValue(normalizeStorePreferenceKey(key), value);
}

export async function appLanguageChanged(language: string) {
    await commands.appLanguageChanged(language);
}

export function normalizeStringList(value: unknown): string[] {
    return Array.isArray(value)
        ? value.map((entry) => String(entry ?? '').trim()).filter(Boolean)
        : [];
}

export function resolveTablePageSize(
    candidate: number,
    pageSizes: readonly number[]
) {
    const allowed = pageSizes;
    const fallbackPageSize = allowed[0] ?? DEFAULT_TABLE_PAGE_SIZE;
    const nearestPageSize = (value: number) =>
        allowed.reduce((previous, size) =>
            Math.abs(size - value) < Math.abs(previous - value)
                ? size
                : previous
        );
    const parsed = normalizeTablePageSize(candidate, fallbackPageSize);
    return allowed.includes(parsed) ? parsed : nearestPageSize(parsed);
}

export {
    normalizePreferenceKey,
    normalizeTableLimits,
    normalizeTablePageSize,
    normalizeTablePageSizes,
    publishPreferenceChanged
};
