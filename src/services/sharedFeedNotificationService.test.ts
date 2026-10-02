import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
    pushNotification: vi.fn()
}));

vi.mock('@/services/i18nService', () => ({
    default: {
        t: (key: string, values: { value: string }) => `${key}:${values.value}`
    }
}));

vi.mock('@/state/notificationStore', () => ({
    useNotificationStore: {
        getState: () => ({ pushNotification: mocks.pushNotification })
    }
}));

import { usePreferencesStore } from '@/state/preferencesStore';

import { pushSharedFeedNotification } from './sharedFeedNotificationService';

describe('pushSharedFeedNotification', () => {
    beforeEach(() => {
        vi.clearAllMocks();
        usePreferencesStore.setState({
            feedHiddenUsers: [],
            feedHiddenUsersHideNotifications: true
        });
    });

    it('skips location changes of hidden friends only while the notification switch is on', async () => {
        usePreferencesStore.setState({ feedHiddenUsers: ['usr_hidden'] });

        await pushSharedFeedNotification({
            type: 'GPS',
            userId: 'usr_hidden',
            displayName: 'Hidden',
            worldName: 'World'
        });
        await pushSharedFeedNotification({
            type: 'Status',
            userId: 'usr_hidden',
            displayName: 'Hidden',
            status: 'busy'
        });
        expect(mocks.pushNotification).toHaveBeenCalledTimes(1);
        expect(mocks.pushNotification).toHaveBeenCalledWith(
            expect.objectContaining({ message: 'Hidden - busy' })
        );

        usePreferencesStore.setState({
            feedHiddenUsersHideNotifications: false
        });
        await pushSharedFeedNotification({
            type: 'GPS',
            userId: 'usr_hidden',
            displayName: 'Hidden',
            worldName: 'World'
        });
        expect(mocks.pushNotification).toHaveBeenCalledTimes(2);
    });

    it('includes the new trust level in the desktop summary', async () => {
        await pushSharedFeedNotification({
            type: 'TrustLevel',
            userId: 'usr_friend',
            displayName: 'Friend',
            trustLevel: 'Trusted User'
        });

        expect(mocks.pushNotification).toHaveBeenCalledWith({
            level: 'info',
            title: 'service.shared_feed_notification_service.dynamic.feed_value:TrustLevel',
            message: 'Friend - Trusted User'
        });
    });
});
