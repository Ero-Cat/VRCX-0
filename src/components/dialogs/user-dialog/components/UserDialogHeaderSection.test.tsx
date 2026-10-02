// @vitest-environment jsdom

import {
    act,
    cleanup,
    fireEvent,
    render,
    screen
} from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { InventoryItemRecord } from '@/repositories/vrchatMediaRepository';

import {
    UserDialogHeaderSection,
    type UserHeaderCommands,
    type UserHeaderModel
} from './UserDialogHeaderSection';

let notifyResize: (() => void) | null = null;

class ResizeObserverMock {
    constructor(callback: ResizeObserverCallback) {
        notifyResize = () => callback([], this as unknown as ResizeObserver);
    }

    observe() {}
    unobserve() {}
    disconnect() {}
}

vi.stubGlobal('ResizeObserver', ResizeObserverMock);

afterEach(cleanup);

const nameplateEffect: InventoryItemRecord = {
    id: 'invt_nameplate',
    metadata: {
        assets: [
            {
                type: 'base',
                url: 'https://example.test/nameplate.webp'
            }
        ]
    }
};

const iconFrame: InventoryItemRecord = {
    id: 'invt_frame',
    metadata: {
        assets: [
            {
                type: 'base',
                url: 'https://example.test/frame.webp'
            }
        ]
    }
};

const profileEffect: InventoryItemRecord = {
    id: 'invt_profile',
    metadata: {
        assets: [
            {
                type: 'base',
                url: 'https://example.test/profile-effect.webp'
            }
        ]
    }
};

function createHeaderModel(effect?: InventoryItemRecord): UserHeaderModel {
    return {
        actionStatus: 'idle',
        appearanceVisibility: {
            profileBackground: true,
            avatarFrame: true,
            profileEffect: true,
            nameplateEffect: true
        },
        avatarOverrideState: {
            hideAvatar: false,
            showAvatar: false
        },
        bannerFallbackUrl: '',
        canInviteFromCurrentLocation: false,
        currentAvatarTarget: '',
        currentUserBoopingEnabled: false,
        detail: '',
        extendedModerationState: {
            interactOff: false,
            muteChat: false
        },
        fallbackAvatarTarget: '',
        friendRequestState: {
            incoming: false,
            outgoing: false
        },
        imageUrl: '',
        isCurrentUser: true,
        isFriend: false,
        loadStatus: 'ready',
        moderationState: {
            block: false,
            mute: false
        },
        platform: {
            icon: null,
            label: 'PC'
        },
        PlatformIcon: null,
        previousDisplayNames: [],
        profile: {
            displayName: 'Map1en_',
            id: 'usr_test'
        },
        profileAppearance: effect ? { nameplateEffect: effect } : {},
        profileIconUrl: '',
        profileLanguages: [],
        profileTitle: 'Map1en_',
        recentDialogShortcut: () => null,
        statusDotClassName: '',
        statusStateText: '',
        username: '',
        userUrl: ''
    };
}

function createHeaderCommands(): UserHeaderCommands {
    const noop = () => undefined;

    return {
        onAvatarOverride: noop,
        onBoop: noop,
        onCopyDisplayName: noop,
        onCopyUserId: noop,
        onCopyUserUrl: noop,
        onEditMemo: noop,
        onEditSelfProfileDetails: noop,
        onEditSelfProfileMedia: noop,
        onEditSelfProfileDecorations: noop,
        onEditSelfStatus: noop,
        onExtendedModeration: noop,
        onFriendRequest: noop,
        onGroupModeration: noop,
        onImageClick: noop,
        onInvite: noop,
        onInviteMessage: noop,
        onInviteRequest: noop,
        onInviteRequestMessage: noop,
        onInviteToGroup: noop,
        onModeration: noop,
        onOpenDiscordProfile: noop,
        onOpenFallbackAvatar: noop,
        onOpenImagePreview: noop,
        onOpenUserIcon: noop,
        onOpenUserUrl: noop,
        onRefresh: noop,
        onReportHacking: noop,
        onShowAvatarAuthor: noop,
        onShowInstanceHistory: noop,
        onToggleBadgeShowcased: noop,
        onToggleBadgeVisibility: noop,
        onToggleSelfAvatarCopying: noop,
        onToggleSelfBooping: noop,
        onToggleSelfDiscordConnections: noop,
        onToggleSelfSharedConnections: noop,
        onUnfriend: noop
    };
}

describe('UserDialogHeaderSection nameplate', () => {
    it('copies the current display name from the title', () => {
        const headerCommands = createHeaderCommands();
        headerCommands.onCopyDisplayName = vi.fn();

        render(
            <UserDialogHeaderSection
                headerModel={createHeaderModel()}
                headerCommands={headerCommands}
            />
        );

        const displayNameButton = screen.getByText('Map1en_').closest('button');
        expect(displayNameButton).not.toBeNull();
        fireEvent.click(displayNameButton as HTMLButtonElement);

        expect(headerCommands.onCopyDisplayName).toHaveBeenCalledOnce();
        expect(headerCommands.onCopyDisplayName).toHaveBeenCalledWith(
            'Map1en_'
        );
    });

    it('shrinks a long display name to the available width', () => {
        const headerModel = createHeaderModel();
        headerModel.profile.displayName = 'A very long display name';
        headerModel.profileTitle = 'A very long display name';

        render(
            <UserDialogHeaderSection
                headerModel={headerModel}
                headerCommands={createHeaderCommands()}
            />
        );

        const displayName = screen.getByText('A very long display name');
        Object.defineProperty(displayName, 'clientWidth', {
            configurable: true,
            value: 150
        });
        Object.defineProperty(displayName, 'scrollWidth', {
            configurable: true,
            value: 180
        });

        act(() => notifyResize?.());

        expect(displayName.style.fontSize).toBe('15px');
    });
});

describe('UserDialogHeaderSection appearance visibility', () => {
    it('hides the profile background without changing the card structure', () => {
        const headerModel = createHeaderModel();
        headerModel.profile.backgroundType = 'gradient';
        headerModel.profile.backgroundGradientTop = '#ff0000';
        headerModel.profile.backgroundGradientBottom = '#0000ff';
        headerModel.appearanceVisibility.profileBackground = false;

        const { container } = render(
            <UserDialogHeaderSection
                headerModel={headerModel}
                headerCommands={createHeaderCommands()}
            />
        );
        const card = container.querySelector<HTMLElement>('[data-slot="card"]');

        expect(card).not.toBeNull();
        expect(card?.style.backgroundImage).toBe('');
    });

    it.each([
        ['avatar frame', 'avatarFrame', false, true],
        ['nameplate effect', 'nameplateEffect', true, false]
    ] as const)(
        'hides only the %s when its visibility is off',
        (_label, hiddenSlot, frameVisible, nameplateVisible) => {
            const headerModel = createHeaderModel(nameplateEffect);
            headerModel.profileAppearance.iconFrame = iconFrame;
            headerModel.profileIconUrl = 'https://example.test/icon.webp';
            headerModel.appearanceVisibility[hiddenSlot] = false;

            const { container } = render(
                <UserDialogHeaderSection
                    headerModel={headerModel}
                    headerCommands={createHeaderCommands()}
                />
            );

            expect(
                container.querySelector(
                    'img[src="https://example.test/frame.webp"]'
                ) !== null
            ).toBe(frameVisible);
            expect(
                container.querySelector(
                    'img[src="https://example.test/nameplate.webp"]'
                ) !== null
            ).toBe(nameplateVisible);
        }
    );

    it('hides the profile effect without hiding the nameplate effect', () => {
        const headerModel = createHeaderModel(nameplateEffect);
        headerModel.profileAppearance.profileEffect = profileEffect;
        headerModel.appearanceVisibility.profileEffect = false;

        const { container } = render(
            <UserDialogHeaderSection
                headerModel={headerModel}
                headerCommands={createHeaderCommands()}
            />
        );

        expect(
            container.querySelector(
                'img[src="https://example.test/profile-effect.webp"]'
            )
        ).toBeNull();
        expect(
            container.querySelector(
                'img[src="https://example.test/nameplate.webp"]'
            )
        ).not.toBeNull();
    });
});

describe('UserDialogHeaderSection friend number', () => {
    it('shows the stored friend number for a current friend', () => {
        const headerModel = createHeaderModel();
        headerModel.friendNumber = 42;
        headerModel.isCurrentUser = false;
        headerModel.isFriend = true;

        render(
            <UserDialogHeaderSection
                headerModel={headerModel}
                headerCommands={createHeaderCommands()}
            />
        );

        expect(screen.getByText('#42')).toBeTruthy();
    });

    it('hides the stored friend number for a former friend', () => {
        const headerModel = createHeaderModel();
        headerModel.friendNumber = 42;
        headerModel.isCurrentUser = false;
        headerModel.isFriend = false;

        render(
            <UserDialogHeaderSection
                headerModel={headerModel}
                headerCommands={createHeaderCommands()}
            />
        );

        expect(screen.queryByText('#42')).toBeNull();
    });
});

describe('UserDialogHeaderSection friend actions', () => {
    it('opens instance history before its rows have been loaded', async () => {
        const headerModel = createHeaderModel();
        headerModel.isCurrentUser = false;
        const headerCommands = createHeaderCommands();
        headerCommands.onShowInstanceHistory = vi.fn();

        render(
            <UserDialogHeaderSection
                headerModel={headerModel}
                headerCommands={headerCommands}
            />
        );

        fireEvent.click(
            screen.getByRole('button', { name: 'Open entity actions' })
        );

        const label = await screen.findByText(
            'dialog.user.actions.show_previous_instances'
        );
        fireEvent.click(label);

        expect(headerCommands.onShowInstanceHistory).toHaveBeenCalledOnce();
    });

    it('offers cancellation for an outgoing friend request', async () => {
        const headerModel = createHeaderModel();
        headerModel.isCurrentUser = false;
        headerModel.friendRequestState.outgoing = true;

        render(
            <UserDialogHeaderSection
                headerModel={headerModel}
                headerCommands={createHeaderCommands()}
            />
        );

        fireEvent.click(
            screen.getByRole('button', { name: 'Open entity actions' })
        );

        expect(
            await screen.findByText('dialog.user.actions.cancel_friend_request')
        ).toBeTruthy();
        expect(
            screen.queryByText('dialog.user.actions.send_friend_request')
        ).toBeNull();
    });

    it('marks unfriend as destructive', async () => {
        const headerModel = createHeaderModel();
        headerModel.isCurrentUser = false;
        headerModel.isFriend = true;

        render(
            <UserDialogHeaderSection
                headerModel={headerModel}
                headerCommands={createHeaderCommands()}
            />
        );

        fireEvent.click(
            screen.getByRole('button', { name: 'Open entity actions' })
        );

        const label = await screen.findByText('dialog.user.actions.unfriend');
        expect(
            label
                .closest('[data-slot="dropdown-menu-item"]')
                ?.getAttribute('data-variant')
        ).toBe('destructive');
    });
});

describe('UserDialogHeaderSection creator badge', () => {
    it('shows the economy creator fact when the profile provides it', () => {
        const headerModel = createHeaderModel();
        headerModel.profile.isEconomyCreator = true;

        render(
            <UserDialogHeaderSection
                headerModel={headerModel}
                headerCommands={createHeaderCommands()}
            />
        );

        expect(
            screen.getByText('dialog.user.label.economy_creator')
        ).toBeTruthy();
    });

    it('hides the economy creator fact when the profile omits it', () => {
        render(
            <UserDialogHeaderSection
                headerModel={createHeaderModel()}
                headerCommands={createHeaderCommands()}
            />
        );

        expect(
            screen.queryByText('dialog.user.label.economy_creator')
        ).toBeNull();
    });
});
