import {
    commands,
    type NotificationActivityFilterSurface,
    type ActivityFilterProfile
} from '@/platform/tauri/bindings';

import { patchPreferences, publishPreferenceChanged } from './preferencesCore';

type ActivityFilterPreferenceKey =
    | 'overlayActivityFilters'
    | 'vrNotificationActivityFilters'
    | 'hmdNotificationActivityFilters'
    | 'desktopNotificationActivityFilters'
    | 'webhookActivityFilters'
    | 'ttsNotificationActivityFilters';

async function setActivityFilterPreference(
    key: ActivityFilterPreferenceKey,
    surface: NotificationActivityFilterSurface,
    filters: ActivityFilterProfile
) {
    const saved = await commands.appNotificationActivityFiltersSet({
        surface,
        filters
    });
    patchPreferences({ [key]: saved });
    publishPreferenceChanged(key, saved);
    return saved;
}

export function setOverlayActivityFiltersPreference(
    value: ActivityFilterProfile
) {
    return setActivityFilterPreference(
        'overlayActivityFilters',
        'wrist',
        value
    );
}

export function setVrNotificationActivityFiltersPreference(
    value: ActivityFilterProfile
) {
    return setActivityFilterPreference(
        'vrNotificationActivityFilters',
        'vr',
        value
    );
}

export function setDesktopNotificationActivityFiltersPreference(
    value: ActivityFilterProfile
) {
    return setActivityFilterPreference(
        'desktopNotificationActivityFilters',
        'desktop',
        value
    );
}

export function setHmdNotificationActivityFiltersPreference(
    value: ActivityFilterProfile
) {
    return setActivityFilterPreference(
        'hmdNotificationActivityFilters',
        'hmd',
        value
    );
}

export function setWebhookActivityFiltersPreference(
    value: ActivityFilterProfile
) {
    return setActivityFilterPreference(
        'webhookActivityFilters',
        'webhook',
        value
    );
}

export function setTtsNotificationActivityFiltersPreference(
    value: ActivityFilterProfile
) {
    return setActivityFilterPreference(
        'ttsNotificationActivityFilters',
        'tts',
        value
    );
}

export async function setWristOverlayEnabledPreference(value: boolean) {
    const snapshot = await commands.appVrOverlayEnabledSet(value);
    const wristOverlayEnabled = snapshot.enabled;
    patchPreferences({ wristOverlayEnabled });
    publishPreferenceChanged('wristOverlayEnabled', wristOverlayEnabled);
    return wristOverlayEnabled;
}
