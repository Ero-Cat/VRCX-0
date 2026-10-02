import { useTranslation } from 'react-i18next';

import { BoopEmojiDialog } from '@/components/dialogs/BoopEmojiDialog';
import { ToolbarFilterMenu } from '@/components/layout/ToolbarControls';
import type { BoopEmojiChoice } from '@/domain/entities/boopEmoji';
import { NOTIFICATION_TYPES } from '@/repositories/notificationPersistenceRepository';
import {
    DropdownMenuCheckboxItem,
    DropdownMenuGroup,
    DropdownMenuLabel
} from '@/ui/shadcn/dropdown-menu';

import type { NotificationRow } from '../notificationPageTypes';
import { sanitizeNotificationFilters } from '../notificationTableState';

const NOTIFICATION_TYPE_SECTIONS: {
    key: string;
    labelKey: string;
    types: string[];
}[] = [
    {
        key: 'social',
        labelKey: 'view.notification.feed.section.social',
        types: [
            'friendRequest',
            'ignoredFriendRequest',
            'invite',
            'requestInvite',
            'inviteResponse',
            'requestInviteResponse',
            'invite.instance.contentGated',
            'message',
            'boop'
        ]
    },
    {
        key: 'group',
        labelKey: 'view.notification.feed.section.group',
        types: [
            'groupChange',
            'group.announcement',
            'group.event.created',
            'group.event.starting',
            'group.post',
            'group.informative',
            'group.invite',
            'group.joinRequest',
            'group.transfer',
            'group.queueReady',
            'event.announcement'
        ]
    },
    {
        key: 'moderation',
        labelKey: 'view.notification.feed.section.moderation',
        types: [
            'moderation.warning.group',
            'moderation.report.closed',
            'moderation.contentrestriction',
            'moderation.notice',
            'votetokick'
        ]
    },
    {
        key: 'other',
        labelKey: 'view.notification.feed.section.other',
        types: [
            'instance.closed',
            'economy.alert',
            'economy.received.gift',
            'badge.earned',
            'vrcplus.gift',
            'avatarreview.success',
            'avatarreview.failure',
            'promo.redeem',
            'twitchdrop.fulfilled',
            'text.adventure'
        ]
    }
];

function sectionedNotificationTypes() {
    const assigned = new Set(
        NOTIFICATION_TYPE_SECTIONS.flatMap((section) => section.types)
    );
    return NOTIFICATION_TYPE_SECTIONS.map((section) => ({
        ...section,
        types:
            section.key === 'other'
                ? [
                      ...section.types.filter((type) =>
                          NOTIFICATION_TYPES.includes(type)
                      ),
                      ...NOTIFICATION_TYPES.filter(
                          (type) => !assigned.has(type)
                      )
                  ]
                : section.types.filter((type) =>
                      NOTIFICATION_TYPES.includes(type)
                  )
    })).filter((section) => section.types.length > 0);
}

export function NotificationTypeFilterDropdown({
    value,
    onChange,
    getTypeLabel = (type) => type
}: {
    getTypeLabel?: (type: string) => string;
    onChange: (value: string[]) => void;
    value: string[];
}) {
    const { t } = useTranslation();

    const activeTypes = value;
    const sections = sectionedNotificationTypes();

    function toggleType(type: string, checked: boolean) {
        const nextTypes = checked
            ? [...activeTypes, type]
            : activeTypes.filter((entry) => entry !== type);
        onChange(sanitizeNotificationFilters(nextTypes, NOTIFICATION_TYPES));
    }

    return (
        <ToolbarFilterMenu
            activeCount={activeTypes.length}
            contentClassName="w-80"
        >
            {sections.map((section) => (
                <DropdownMenuGroup key={section.key}>
                    <DropdownMenuLabel>{t(section.labelKey)}</DropdownMenuLabel>
                    {section.types.map((type) => (
                        <DropdownMenuCheckboxItem
                            key={type}
                            checked={activeTypes.includes(type)}
                            onCheckedChange={(checked) =>
                                toggleType(type, checked)
                            }
                            onClick={(event) => event.preventDefault()}
                        >
                            {getTypeLabel(type)}
                        </DropdownMenuCheckboxItem>
                    ))}
                </DropdownMenuGroup>
            ))}
        </ToolbarFilterMenu>
    );
}

export function BoopReplyDialog({
    request,
    isLocalUserVrcPlusSupporter,
    onOpenChange,
    onSend
}: {
    isLocalUserVrcPlusSupporter: boolean;
    onOpenChange: (open: boolean) => void;
    onSend: (
        notification: NotificationRow | null,
        emoji: BoopEmojiChoice | null
    ) => void | Promise<void>;
    request: NotificationRow | null;
}) {
    const open = Boolean(request);
    const notification = request || null;
    const displayName = notification?.senderUsername || 'this user';
    return (
        <BoopEmojiDialog
            open={open}
            isLocalUserVrcPlusSupporter={isLocalUserVrcPlusSupporter}
            targetLabel={displayName}
            sendDisabled={!notification?.senderUserId}
            onOpenChange={onOpenChange}
            onSend={(emoji) => onSend(notification, emoji)}
        />
    );
}
