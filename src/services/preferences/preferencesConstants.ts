import {
    DEFAULT_PREFERENCES,
    type DiscordPreferenceKey
} from '@/state/preferencesStore';

export const DEFAULT_NOTIFICATION_LAYOUT = 'notification-center';

export const DEFAULT_TABLE_PAGE_SIZE =
    Number(DEFAULT_PREFERENCES.tablePageSize) || 20;
export const DEFAULT_TABLE_PAGE_SIZES = Array.isArray(
    DEFAULT_PREFERENCES.tablePageSizes
)
    ? DEFAULT_PREFERENCES.tablePageSizes
    : [10, 15, 20, 25, 50, 100];
export const DEFAULT_TABLE_LIMITS = DEFAULT_PREFERENCES.tableLimits;
export const DISCORD_BOOL_PREFERENCE_KEYS = new Set<DiscordPreferenceKey>([
    'discordActive',
    'discordInstance',
    'discordHideInvite',
    'discordJoinButton',
    'discordHideImage',
    'discordShowPlatform',
    'discordWorldIntegration',
    'discordWorldNameAsDiscordStatus'
]);
