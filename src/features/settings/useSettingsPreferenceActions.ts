import { commands } from '@/platform/tauri/bindings';
import type {
    BoolConfigPreferenceKey,
    StringConfigPreferenceKey
} from '@/services/preferencesService';
import {
    consumeSystemFontsUnavailableWarning,
    loadSystemFonts
} from '@/services/systemFontsService';
import type { AppToastOptions } from '@/services/toastService';
import { loadVrchatConfigSnapshot } from '@/services/vrchatConfigService';
import type {
    PreferencesSnapshot,
    PreferencesStoreState,
    TrustColorKey
} from '@/state/preferencesStore';

import {
    composeCustomFontFamily,
    createCustomFontDraftFromPrefs,
    type CustomFontDraft
} from './settingsValues';

type PreferenceKey = Extract<keyof PreferencesSnapshot, string>;
type NormalizedConfigKey<Key extends string> = Key extends `VRCX_${infer Name}`
    ? Name
    : Key;
type BoolPreferenceKey = NormalizedConfigKey<BoolConfigPreferenceKey> &
    PreferenceKey;
type StringPreferenceKey = NormalizedConfigKey<StringConfigPreferenceKey> &
    PreferenceKey;
type PreferenceAction = () => void;
type PreferenceRollback = void | (() => void);
export type SettingsActionPrefs = PreferencesSnapshot;
type SettingsPrefs = SettingsActionPrefs;
type StateSetter<Value> = (value: Value | ((current: Value) => Value)) => void;
type SettingsPreferenceActionsDeps = {
    APP_FONT_DEFAULT_KEY: string;
    DEFAULT_MAX_TABLE_SIZE: number;
    DEFAULT_SEARCH_LIMIT: number;
    applyAppFontPreferences: (preferences: {
        fontFamily: string;
        customFontFamily: string;
        cjkFontPack: string;
    }) => void;
    auth: {
        currentUserEndpoint?: string | null;
        currentUserId?: string | null;
    };
    commit: (
        action: PreferenceAction,
        optimistic?: () => PreferenceRollback
    ) => Promise<boolean>;
    configRepository: {
        setMany(entries: Array<[string, string]>): Promise<void>;
    };
    customFontDraft: CustomFontDraft;
    isValidFontFamilyList: (value: string) => boolean;
    loadTrustColorPreference: () => Promise<PreferencesSnapshot['trustColor']>;
    localFavoriteFriendsGroups: string[];
    normalizeAppCjkFontPack: (value: string) => string;
    normalizeAppFontFamily: (value: string) => string;
    parseIntegerInput: (value: string | number, fallback: number) => number;
    prefs: SettingsPrefs;
    resetTrustColorsPreference: () => Promise<
        PreferencesSnapshot['trustColor']
    >;
    setBoolConfigPreference: (
        key: BoolConfigPreferenceKey,
        value: boolean
    ) => Promise<void>;
    setConfigTreeData: (value: Record<string, unknown>) => void;
    setCustomFontDialogOpen: (value: boolean) => void;
    setCustomFontDraft: (value: CustomFontDraft) => void;
    setCustomFontOptions: (value: string[]) => void;
    setCustomFontOptionsLoading: (value: boolean) => void;
    setLocalFavoriteFriendsGroups: (value: string[]) => void;
    setLocalFavoriteFriendsGroupsPreference: (
        value: string[]
    ) => Promise<string[]>;
    setOnlineVisitCount: (value: number) => void;
    setPrefs: StateSetter<SettingsPrefs>;
    setProxyEnabledPreference: (value: boolean) => Promise<boolean>;
    setSqliteTableSizes: (value: Record<string, unknown>) => void;
    setStringConfigPreference: (
        key: StringConfigPreferenceKey,
        value: string
    ) => Promise<void>;
    setTableLimitsDialogOpen: (value: boolean) => void;
    setTableLimitsDraft: (value: {
        maxTableSize: string;
        searchLimit: string;
    }) => void;
    setTableLimitsPreference: (value: {
        maxTableSize: number;
        searchLimit: number;
    }) => Promise<PreferencesSnapshot['tableLimits']>;
    setTablePageSizesDialogOpen: (value: boolean) => void;
    setTrustColorPreference: (
        key: TrustColorKey,
        value: string
    ) => Promise<PreferencesSnapshot['trustColor']>;
    setOverlayActivityFiltersPreference: (
        value: PreferencesSnapshot['overlayActivityFilters']
    ) => Promise<PreferencesSnapshot['overlayActivityFilters']>;
    setVrNotificationActivityFiltersPreference: (
        value: PreferencesSnapshot['vrNotificationActivityFilters']
    ) => Promise<PreferencesSnapshot['vrNotificationActivityFilters']>;
    setHmdNotificationActivityFiltersPreference: (
        value: PreferencesSnapshot['hmdNotificationActivityFilters']
    ) => Promise<PreferencesSnapshot['hmdNotificationActivityFilters']>;
    setDesktopNotificationActivityFiltersPreference: (
        value: PreferencesSnapshot['desktopNotificationActivityFilters']
    ) => Promise<PreferencesSnapshot['desktopNotificationActivityFilters']>;
    setWebhookActivityFiltersPreference: (
        value: PreferencesSnapshot['webhookActivityFilters']
    ) => Promise<PreferencesSnapshot['webhookActivityFilters']>;
    setTtsNotificationActivityFiltersPreference: (
        value: PreferencesSnapshot['ttsNotificationActivityFilters']
    ) => Promise<PreferencesSnapshot['ttsNotificationActivityFilters']>;
    setWristOverlayEnabledPreference: (value: boolean) => Promise<boolean>;
    t: (key: string) => string;
    tableLimitsDraft: {
        maxTableSize: string;
        searchLimit: string;
    };
    tableLimitsSaveDisabled: boolean;
    toast: {
        add(options: AppToastOptions): void;
    };
    usePreferencesStore: {
        getState(): Pick<PreferencesStoreState, 'proxyServer' | 'tableLimits'>;
    };
    vrchatAuthRepository: {
        getOnlineVisits(): Promise<{ json: unknown }>;
    };
};

type FontPreferencesInput = Partial<{
    cjkFontPack: string;
    customFontFamily: string;
    fontFamily: string;
}>;

type ActivityFilterSurfaceField =
    | 'overlayActivityFilters'
    | 'vrNotificationActivityFilters'
    | 'hmdNotificationActivityFilters'
    | 'desktopNotificationActivityFilters'
    | 'webhookActivityFilters'
    | 'ttsNotificationActivityFilters';

type ActivityFilterSurfaceSetter<Field extends ActivityFilterSurfaceField> = (
    value: PreferencesSnapshot[Field]
) => Promise<PreferencesSnapshot[Field]>;

export function useSettingsPreferenceActions({
    APP_FONT_DEFAULT_KEY,
    DEFAULT_MAX_TABLE_SIZE,
    DEFAULT_SEARCH_LIMIT,
    applyAppFontPreferences,
    auth,
    commit,
    configRepository,
    customFontDraft,
    isValidFontFamilyList,
    loadTrustColorPreference,
    localFavoriteFriendsGroups,
    normalizeAppCjkFontPack,
    normalizeAppFontFamily,
    parseIntegerInput,
    prefs,
    resetTrustColorsPreference,
    setBoolConfigPreference,
    setConfigTreeData,
    setCustomFontDialogOpen,
    setCustomFontDraft,
    setCustomFontOptions,
    setCustomFontOptionsLoading,
    setLocalFavoriteFriendsGroups,
    setLocalFavoriteFriendsGroupsPreference,
    setOnlineVisitCount,
    setPrefs,
    setProxyEnabledPreference,
    setSqliteTableSizes,
    setStringConfigPreference,
    setTableLimitsDialogOpen,
    setTableLimitsDraft,
    setTableLimitsPreference,
    setTablePageSizesDialogOpen,
    setTrustColorPreference,
    setOverlayActivityFiltersPreference,
    setVrNotificationActivityFiltersPreference,
    setHmdNotificationActivityFiltersPreference,
    setDesktopNotificationActivityFiltersPreference,
    setWebhookActivityFiltersPreference,
    setTtsNotificationActivityFiltersPreference,
    setWristOverlayEnabledPreference,
    t,
    tableLimitsDraft,
    tableLimitsSaveDisabled,
    toast,
    usePreferencesStore,
    vrchatAuthRepository
}: SettingsPreferenceActionsDeps) {
    async function savePreferenceValue<K extends PreferenceKey>(
        key: K,
        value: PreferencesSnapshot[K],
        action: PreferenceAction
    ) {
        return commit(action, () => {
            const previous = prefs[key];
            setPrefs((current) => ({
                ...current,
                [key]: value
            }));
            return () =>
                setPrefs((current) => ({
                    ...current,
                    [key]: previous
                }));
        });
    }
    async function saveBoolPreference(
        key: BoolPreferenceKey,
        configKey: BoolConfigPreferenceKey,
        value: boolean
    ) {
        const enabled = value === true;
        await savePreferenceValue(key, enabled, () =>
            setBoolConfigPreference(configKey, enabled)
        );
    }
    async function saveStringPreference(
        key: StringPreferenceKey,
        configKey: StringConfigPreferenceKey,
        value: string
    ) {
        await savePreferenceValue(key, value, () =>
            setStringConfigPreference(configKey, value)
        );
    }
    async function saveFontPreferences({
        fontFamily = prefs.appFontFamily,
        cjkFontPack = prefs.appCjkFontPack,
        customFontFamily = prefs.customFontFamily
    }: FontPreferencesInput = {}) {
        const nextFontFamily = normalizeAppFontFamily(fontFamily);
        const nextCjkFontPack = normalizeAppCjkFontPack(cjkFontPack);
        await configRepository.setMany([
            ['VRCX_fontFamily', nextFontFamily],
            ['VRCX_cjkFontPack', nextCjkFontPack]
        ]);
        setPrefs((current) => ({
            ...current,
            appFontFamily: nextFontFamily,
            appCjkFontPack: nextCjkFontPack
        }));
        applyAppFontPreferences({
            fontFamily: nextFontFamily,
            customFontFamily,
            cjkFontPack: nextCjkFontPack
        });
    }
    async function saveFontFamilyPreference(
        fontFamily: string,
        customFontFamily: string = prefs.customFontFamily
    ) {
        await saveFontPreferences({
            fontFamily,
            customFontFamily
        });
    }
    async function selectCjkFontPack(cjkFontPack: string) {
        await saveFontPreferences({
            fontFamily:
                prefs.appFontFamily === 'custom'
                    ? APP_FONT_DEFAULT_KEY
                    : prefs.appFontFamily,
            cjkFontPack
        });
    }
    function openCustomFontDialog() {
        setCustomFontDraft(createCustomFontDraftFromPrefs(prefs));
        setCustomFontDialogOpen(true);
        setCustomFontOptionsLoading(true);
        loadSystemFonts()
            .then((fonts) => {
                setCustomFontOptions(fonts);
                if (consumeSystemFontsUnavailableWarning(fonts)) {
                    toast.add({
                        type: 'warning',
                        title: t(
                            'view.settings.appearance.appearance.font_family_custom_detection_unavailable_toast'
                        )
                    });
                }
            })
            .finally(() => {
                setCustomFontOptionsLoading(false);
            });
    }
    async function saveCustomFontFamily(
        value: CustomFontDraft = customFontDraft
    ) {
        const draft = value;
        const nextDraft: CustomFontDraft = {
            primary: String(draft.primary ?? '').trim(),
            secondary: String(draft.secondary ?? '').trim(),
            override: String(draft.override ?? '').trim()
        };
        const nextValue = composeCustomFontFamily(nextDraft);
        if (!isValidFontFamilyList(nextValue)) {
            toast.add({
                type: 'error',
                title: t(
                    'view.settings.appearance.appearance.font_family_custom_invalid'
                )
            });
            return;
        }
        const previousFontFamily = prefs.appFontFamily;
        const previousCustomFontFamily = prefs.customFontFamily;
        const previousCustomFontPrimary = prefs.customFontPrimary;
        const previousCustomFontSecondary = prefs.customFontSecondary;
        const previousCustomFontOverride = prefs.customFontOverride;
        const saved = await commit(
            () =>
                configRepository.setMany([
                    ['customFontPrimary', nextDraft.primary],
                    ['customFontSecondary', nextDraft.secondary],
                    ['customFontOverride', nextDraft.override],
                    ['customFontFamily', nextValue],
                    ['VRCX_fontFamily', 'custom']
                ]),
            () => {
                setPrefs((current) => ({
                    ...current,
                    appFontFamily: 'custom',
                    customFontFamily: nextValue,
                    customFontPrimary: nextDraft.primary,
                    customFontSecondary: nextDraft.secondary,
                    customFontOverride: nextDraft.override
                }));
                applyAppFontPreferences({
                    fontFamily: 'custom',
                    customFontFamily: nextValue,
                    cjkFontPack: prefs.appCjkFontPack
                });
                return () => {
                    setPrefs((current) => ({
                        ...current,
                        appFontFamily: previousFontFamily,
                        customFontFamily: previousCustomFontFamily,
                        customFontPrimary: previousCustomFontPrimary,
                        customFontSecondary: previousCustomFontSecondary,
                        customFontOverride: previousCustomFontOverride
                    }));
                    applyAppFontPreferences({
                        fontFamily: previousFontFamily,
                        customFontFamily: previousCustomFontFamily,
                        cjkFontPack: prefs.appCjkFontPack
                    });
                };
            }
        );
        if (!saved) {
            return;
        }
        setCustomFontDialogOpen(false);
        toast.add({ type: 'success', title: t('common.settings_saved') });
    }
    async function restorePersistedTrustColors() {
        const persisted = await loadTrustColorPreference();
        setPrefs((current) => ({
            ...current,
            trustColor: persisted
        }));
    }
    async function saveTrustColor(key: TrustColorKey, value: string) {
        try {
            const nextTrustColor = await setTrustColorPreference(key, value);
            setPrefs((current) => ({
                ...current,
                trustColor: nextTrustColor
            }));
        } catch (error) {
            toast.add({
                type: 'error',
                title:
                    error instanceof Error
                        ? error.message
                        : t('view.settings.toast.failed_to_save_trust_color')
            });
            await restorePersistedTrustColors();
        }
    }
    async function resetTrustColors() {
        try {
            const nextTrustColor = await resetTrustColorsPreference();
            setPrefs((current) => ({
                ...current,
                trustColor: nextTrustColor
            }));
            toast.add({ type: 'success', title: t('common.settings_saved') });
        } catch (error) {
            toast.add({
                type: 'error',
                title:
                    error instanceof Error
                        ? error.message
                        : t('view.settings.toast.failed_to_save_trust_color')
            });
        }
    }
    async function refreshSqliteTableSizes() {
        try {
            const sizes = await commands.appDatabaseMaintenanceTableSizesGet(
                auth.currentUserId || ''
            );
            setSqliteTableSizes({
                gps: sizes.gps,
                status: sizes.status,
                bio: sizes.bio,
                avatar: sizes.avatar,
                onlineOffline: sizes.onlineOffline,
                friendLogHistory: sizes.friendLogHistory,
                notification: sizes.notification,
                location: sizes.location,
                joinLeave: sizes.joinLeave,
                portalSpawn: sizes.portalSpawn,
                videoPlay: sizes.videoPlay,
                event: sizes.event,
                external: sizes.external
            });
        } catch (error) {
            toast.add({
                type: 'error',
                title:
                    error instanceof Error
                        ? error.message
                        : t(
                              'view.settings.toast.failed_to_refresh_sqlite_table_sizes'
                          )
            });
        }
    }
    async function refreshConfigTreeData() {
        try {
            const snapshot = await loadVrchatConfigSnapshot({ force: true });
            setConfigTreeData(snapshot || {});
        } catch (error) {
            toast.add({
                type: 'error',
                title:
                    error instanceof Error
                        ? error.message
                        : t('view.settings.toast.failed_to_refresh_config_json')
            });
        }
    }
    async function refreshOnlineVisits() {
        try {
            const response = await vrchatAuthRepository.getOnlineVisits();
            setOnlineVisitCount(Number(response.json) || 0);
        } catch (error) {
            toast.add({
                type: 'error',
                title:
                    error instanceof Error
                        ? error.message
                        : t(
                              'view.settings.toast.failed_to_refresh_online_user_count'
                          )
            });
        }
    }
    async function openTablePageSizesDialog() {
        setTablePageSizesDialogOpen(true);
    }
    async function openTableLimitsDialog() {
        const { maxTableSize, searchLimit } =
            usePreferencesStore.getState().tableLimits;
        setTableLimitsDraft({
            maxTableSize: String(
                parseIntegerInput(maxTableSize, DEFAULT_MAX_TABLE_SIZE)
            ),
            searchLimit: String(
                parseIntegerInput(searchLimit, DEFAULT_SEARCH_LIMIT)
            )
        });
        setTableLimitsDialogOpen(true);
    }
    async function saveTableLimitsDialog() {
        if (tableLimitsSaveDisabled) {
            return;
        }
        const nextMaxTableSize = Number.parseInt(
            tableLimitsDraft.maxTableSize,
            10
        );
        const nextSearchLimit = Number.parseInt(
            tableLimitsDraft.searchLimit,
            10
        );
        let savedLimits = prefs.tableLimits;
        const saved = await commit(async () => {
            savedLimits = await setTableLimitsPreference({
                maxTableSize: nextMaxTableSize,
                searchLimit: nextSearchLimit
            });
        });
        if (!saved) {
            return;
        }
        setPrefs((current) => ({
            ...current,
            tableLimits: savedLimits
        }));
        setTableLimitsDialogOpen(false);
        toast.add({ type: 'success', title: t('common.settings_saved') });
    }
    async function toggleLocalFavoriteFriendsGroup(
        groupKey: string,
        checked: boolean
    ) {
        const previousGroups = localFavoriteFriendsGroups;
        const nextGroups = checked
            ? Array.from(new Set([...localFavoriteFriendsGroups, groupKey]))
            : localFavoriteFriendsGroups.filter((value) => value !== groupKey);
        await commit(
            () => setLocalFavoriteFriendsGroupsPreference(nextGroups),
            () => {
                setLocalFavoriteFriendsGroups(nextGroups);
                return () => {
                    setLocalFavoriteFriendsGroups(previousGroups);
                };
            }
        );
    }
    function makeSaveActivityFilterSurface<
        Field extends ActivityFilterSurfaceField
    >(field: Field, setPreference: ActivityFilterSurfaceSetter<Field>) {
        return async function saveActivityFilterSurface(
            value: PreferencesSnapshot[Field]
        ) {
            let savedFilters = prefs[field];
            const previousFilters = prefs[field];
            const saved = await commit(
                async () => {
                    savedFilters = await setPreference(value);
                },
                () => {
                    setPrefs((current) => ({
                        ...current,
                        [field]: value
                    }));
                    return () =>
                        setPrefs((current) => ({
                            ...current,
                            [field]: previousFilters
                        }));
                }
            );
            if (!saved) {
                return null;
            }
            setPrefs((current) => ({
                ...current,
                [field]: savedFilters
            }));
            toast.add({ type: 'success', title: t('common.settings_saved') });
            return savedFilters;
        };
    }
    const saveOverlayActivityFilters = makeSaveActivityFilterSurface(
        'overlayActivityFilters',
        setOverlayActivityFiltersPreference
    );
    const saveVrNotificationActivityFilters = makeSaveActivityFilterSurface(
        'vrNotificationActivityFilters',
        setVrNotificationActivityFiltersPreference
    );
    const saveHmdNotificationActivityFilters = makeSaveActivityFilterSurface(
        'hmdNotificationActivityFilters',
        setHmdNotificationActivityFiltersPreference
    );
    const saveDesktopNotificationActivityFilters =
        makeSaveActivityFilterSurface(
            'desktopNotificationActivityFilters',
            setDesktopNotificationActivityFiltersPreference
        );
    const saveWebhookActivityFilters = makeSaveActivityFilterSurface(
        'webhookActivityFilters',
        setWebhookActivityFiltersPreference
    );
    const saveTtsNotificationActivityFilters = makeSaveActivityFilterSurface(
        'ttsNotificationActivityFilters',
        setTtsNotificationActivityFiltersPreference
    );
    async function saveWristOverlayEnabled(value: boolean) {
        let savedValue = value === true;
        const previousValue = prefs.wristOverlayEnabled;
        const saved = await commit(
            async () => {
                savedValue = await setWristOverlayEnabledPreference(savedValue);
            },
            () => {
                setPrefs((current) => ({
                    ...current,
                    wristOverlayEnabled: savedValue
                }));
                return () =>
                    setPrefs((current) => ({
                        ...current,
                        wristOverlayEnabled: previousValue
                    }));
            }
        );
        if (!saved) {
            return null;
        }
        setPrefs((current) => ({
            ...current,
            wristOverlayEnabled: savedValue
        }));
        return savedValue;
    }
    function speakNotificationTts(
        text: string,
        voiceId: string = prefs.notificationTTSVoiceNative
    ) {
        commands
            .appHostTtsSpeak(text, voiceId || null, prefs.notificationTTSVolume)
            .catch((error) => {
                console.warn('Failed to play notification TTS', error);
                toast.add({
                    type: 'warning',
                    title: t(
                        'view.settings.notifications.notifications.text_to_speech.tts_test_failed'
                    )
                });
            });
    }
    return {
        commit,
        savePreferenceValue,
        saveBoolPreference,
        saveStringPreference,
        saveFontFamilyPreference,
        selectCjkFontPack,
        openCustomFontDialog,
        saveCustomFontFamily,
        saveTrustColor,
        resetTrustColors,
        refreshSqliteTableSizes,
        refreshConfigTreeData,
        refreshOnlineVisits,
        setProxyEnabledPreference,
        openTablePageSizesDialog,
        openTableLimitsDialog,
        saveTableLimitsDialog,
        toggleLocalFavoriteFriendsGroup,
        saveOverlayActivityFilters,
        saveVrNotificationActivityFilters,
        saveHmdNotificationActivityFilters,
        saveDesktopNotificationActivityFilters,
        saveWebhookActivityFilters,
        saveTtsNotificationActivityFilters,
        saveWristOverlayEnabled,
        speakNotificationTts
    };
}
