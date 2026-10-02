import i18n from '@/services/i18nService';
import { normalizeString } from '@/shared/utils/string';
import { useNotificationStore } from '@/state/notificationStore';
import { usePreferencesStore } from '@/state/preferencesStore';

type SharedFeedNotificationEntry = Record<string, unknown> & {
    type?: string;
    userId?: string;
    displayName?: string;
    worldName?: string | null;
    avatarName?: string;
    videoName?: string;
    notyName?: string;
    message?: string;
    status?: string;
    statusDescription?: string;
    trustLevel?: string;
};

export async function pushSharedFeedNotification(
    entry?: SharedFeedNotificationEntry | null
): Promise<void> {
    const type = normalizeString(entry?.type) || 'Feed';
    const { feedHiddenUsers, feedHiddenUsersHideNotifications } =
        usePreferencesStore.getState();
    if (
        type === 'GPS' &&
        feedHiddenUsersHideNotifications &&
        feedHiddenUsers.includes(normalizeString(entry?.userId))
    ) {
        return;
    }
    const displayName =
        normalizeString(entry?.displayName || entry?.userId) || 'Unknown';
    const detail =
        entry?.worldName ||
        entry?.avatarName ||
        entry?.videoName ||
        entry?.notyName ||
        entry?.message ||
        entry?.status ||
        entry?.statusDescription ||
        entry?.trustLevel ||
        '';
    useNotificationStore.getState().pushNotification({
        level: 'info',
        title: i18n.t(
            'service.shared_feed_notification_service.dynamic.feed_value',
            { value: type }
        ),
        message: [displayName, detail].filter(Boolean).join(' - ')
    });
}
