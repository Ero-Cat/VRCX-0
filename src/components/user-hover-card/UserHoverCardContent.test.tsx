import { renderToStaticMarkup } from 'react-dom/server';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const hoverCardData = vi.hoisted(() => {
    const createModel = () => ({
        variant: 'profile-only',
        displayName: 'Alice',
        avatarUrl: '',
        avatarPreviewUrl: '',
        userColour: '',
        trustSource: {},
        trustKey: '',
        statusKey: '',
        statusDotClassName: '',
        statusDescription: '',
        note: '',
        onlineForMs: 0,
        instanceEpoch: 0,
        lastOnlineAgoMs: 0,
        location: {
            effectiveLocation: '',
            worldId: '',
            instanceId: '',
            tag: '',
            accessTypeName: '',
            isRealInstance: false,
            isTraveling: false
        }
    });

    return {
        createModel,
        model: createModel()
    };
});

const STATUS_ONLINE_CLASS = 'border-[var(--status-online)]';

vi.mock('react-i18next', () => ({
    initReactI18next: {
        type: '3rdParty',
        init: () => {}
    },
    useTranslation: () => ({
        t: (key: string) => key
    })
}));

vi.mock('@/services/dialogService', () => ({
    openUserDialog: vi.fn(),
    openWorldDialog: vi.fn()
}));

vi.mock('./useUserHoverCardData', () => ({
    useUserHoverCardData: () => ({
        model: hoverCardData.model,
        worldThumb: '',
        population: null,
        populationLoading: false,
        memo: '',
        trustColor: false,
        instanceEpoch: 0
    })
}));

vi.mock('./UserHoverCardMutuals', () => ({
    UserHoverCardMutuals: () => null
}));

import { UserHoverCardContent } from './UserHoverCardContent';

function countOccurrences(text: string, needle: string): number {
    return text.split(needle).length - 1;
}

describe('UserHoverCardContent', () => {
    beforeEach(() => {
        Object.assign(hoverCardData.model, hoverCardData.createModel());
    });

    it('does not render an online status dot for profile-only cards', () => {
        hoverCardData.model.statusKey = '';
        hoverCardData.model.statusDotClassName = '';
        const html = renderToStaticMarkup(
            <UserHoverCardContent userId="usr_1" />
        );

        expect(html).not.toContain('status-online');
    });

    it('shows the active status with avatar and inline status dots', () => {
        hoverCardData.model.statusKey = 'dialog.user.status.active';
        hoverCardData.model.statusDotClassName = `${STATUS_ONLINE_CLASS} bg-background`;

        const html = renderToStaticMarkup(
            <UserHoverCardContent userId="usr_1" />
        );

        expect(html).toContain('dialog.user.status.active');
        expect(countOccurrences(html, STATUS_ONLINE_CLASS)).toBe(2);
    });

    it('shows the signature without the inline active status when present', () => {
        hoverCardData.model.statusKey = 'dialog.user.status.active';
        hoverCardData.model.statusDotClassName = `${STATUS_ONLINE_CLASS} bg-background`;
        hoverCardData.model.statusDescription = 'Building worlds tonight';

        const html = renderToStaticMarkup(
            <UserHoverCardContent userId="usr_1" />
        );

        expect(html).toContain('Building worlds tonight');
        expect(html).not.toContain('dialog.user.status.active');
        expect(countOccurrences(html, STATUS_ONLINE_CLASS)).toBe(1);
    });

    it('keeps the offline signature above the last-online line', () => {
        hoverCardData.model.variant = 'offline';
        hoverCardData.model.statusDotClassName = 'bg-[var(--status-offline)]';
        hoverCardData.model.statusDescription = 'Back next week';
        hoverCardData.model.lastOnlineAgoMs = 120_000;

        const html = renderToStaticMarkup(
            <UserHoverCardContent userId="usr_1" />
        );

        expect(html).toContain('Back next week');
        expect(html).toContain('user_hover_card.last_online');
        expect(html.indexOf('Back next week')).toBeLessThan(
            html.indexOf('user_hover_card.last_online')
        );
    });
});
