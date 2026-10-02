import { describe, expect, it } from 'vitest';

import {
    avatarFeedEntry,
    bioFeedEntry,
    gpsFeedEntry,
    statusFeedEntry
} from '@/components/feed/feedLiveTestEntries';
import type { FeedLiveEntryPayload } from '@/state/feedLiveTypes';

import { mergeFeedRowsWithSnapshot } from './feedLiveMergeService';

function projectLiveEntry(entry: FeedLiveEntryPayload) {
    const result = mergeFeedRowsWithSnapshot({
        buildMergeOptions: ({ rows }) => ({
            rows,
            userId: 'usr_self',
            maxRows: 10
        }),
        liveEntries: [{ sequence: 1, entry }],
        livePatches: [],
        minLiveSequence: 0,
        rows: []
    });
    expect(result.rows).toHaveLength(1);
    return result.rows[0];
}

describe('mergeFeedRowsWithSnapshot', () => {
    it('projects live avatar entries into feed rows', () => {
        expect(
            projectLiveEntry(
                avatarFeedEntry({
                    ownerId: 'usr_author_new',
                    previousOwnerId: 'usr_author_old',
                    avatarName: 'New Avatar',
                    previousAvatarName: 'Old Avatar',
                    currentAvatarImageUrl:
                        'https://api.vrchat.cloud/api/1/image/file_new/1/256',
                    previousCurrentAvatarImageUrl:
                        'https://api.vrchat.cloud/api/1/image/file_old/1/256'
                })
            )
        ).toMatchObject({
            type: 'Avatar',
            userId: 'usr_friend',
            displayName: 'Friend',
            ownerId: 'usr_author_new',
            previousOwnerId: 'usr_author_old',
            avatarName: 'New Avatar',
            previousAvatarName: 'Old Avatar',
            currentAvatarImageUrl:
                'https://api.vrchat.cloud/api/1/image/file_new/1/256',
            previousCurrentAvatarImageUrl:
                'https://api.vrchat.cloud/api/1/image/file_old/1/256'
        });
    });

    it('projects live bio entries into feed rows', () => {
        expect(
            projectLiveEntry(
                bioFeedEntry({ bio: 'new bio', previousBio: 'old bio' })
            )
        ).toMatchObject({
            type: 'Bio',
            userId: 'usr_friend',
            displayName: 'Friend',
            bio: 'new bio',
            previousBio: 'old bio'
        });
    });

    it('drops only the location changes of location-hidden users', () => {
        const result = mergeFeedRowsWithSnapshot({
            buildMergeOptions: ({ rows }) => ({
                rows,
                userId: 'usr_self',
                locationHiddenUserIds: ['usr_friend'],
                maxRows: 10
            }),
            liveEntries: [
                { sequence: 1, entry: gpsFeedEntry() },
                { sequence: 2, entry: statusFeedEntry() },
                {
                    sequence: 3,
                    entry: gpsFeedEntry({ userId: 'usr_other' })
                }
            ],
            livePatches: [],
            minLiveSequence: 0,
            rows: []
        });

        expect(result.rows.map((row) => [row.userId, row.type])).toEqual([
            ['usr_other', 'GPS'],
            ['usr_friend', 'Status']
        ]);
    });
});
