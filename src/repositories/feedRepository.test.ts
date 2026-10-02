import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
    ensureUserTables: vi.fn(),
    getInt: vi.fn(),
    lookupFeedDatabase: vi.fn(),
    queryFeedLatest: vi.fn(),
    searchFeedDatabase: vi.fn()
}));

vi.mock('./configRepository', () => ({
    default: {
        getInt: mocks.getInt
    }
}));

vi.mock('./feedPersistenceRepository', () => ({
    default: {
        lookupFeedDatabase: mocks.lookupFeedDatabase,
        queryFeedLatest: mocks.queryFeedLatest,
        searchFeedDatabase: mocks.searchFeedDatabase
    }
}));

vi.mock('./userSessionRepository', () => ({
    default: {
        ensureUserTables: mocks.ensureUserTables
    }
}));

import feedRepository from './feedRepository';

describe('feedRepository', () => {
    beforeEach(() => {
        vi.clearAllMocks();
        mocks.getInt.mockImplementation((key: string) =>
            Promise.resolve(key === 'searchLimit' ? 50_000 : 500)
        );
        mocks.ensureUserTables.mockResolvedValue({
            userId: 'usr_feed_limit',
            userPrefix: 'usrfeedlimit'
        });
        mocks.lookupFeedDatabase.mockResolvedValue([]);
        mocks.queryFeedLatest.mockResolvedValue({
            rows: [],
            maxSequence: 0
        });
        mocks.searchFeedDatabase.mockResolvedValue([]);
    });

    it('honors an explicit persistence read limit', async () => {
        await feedRepository.queryFeedLatest({
            userId: 'usr_feed_limit',
            maxRows: 80
        });

        expect(mocks.queryFeedLatest).toHaveBeenCalledWith(
            expect.objectContaining({
                maxRows: 80
            })
        );
    });

    it('routes search through the dedicated persistence query', async () => {
        await feedRepository.queryFeed({
            userId: 'usr_feed_limit',
            search: 'needle',
            maxEntries: 80
        });

        expect(mocks.searchFeedDatabase).toHaveBeenCalledWith(
            'needle',
            [],
            [],
            80,
            '',
            '',
            'usr_feed_limit',
            [],
            [],
            false,
            []
        );
        expect(mocks.queryFeedLatest).not.toHaveBeenCalled();
    });

    it('uses the configured search limit when the caller does not override it', async () => {
        await feedRepository.queryFeed({
            userId: 'usr_feed_limit',
            search: 'needle'
        });

        expect(mocks.searchFeedDatabase).toHaveBeenCalledWith(
            'needle',
            [],
            [],
            50_000,
            '',
            '',
            'usr_feed_limit',
            [],
            [],
            false,
            []
        );
    });

    it('uses the configured search limit for a selected friend scope', async () => {
        await feedRepository.queryFeed({
            userId: 'usr_feed_limit',
            scopedUserIds: ['usr_friend']
        });

        expect(mocks.lookupFeedDatabase).toHaveBeenCalledWith(
            'usr_feed_limit',
            [],
            [],
            50_000,
            null,
            [],
            ['usr_friend'],
            []
        );
        expect(mocks.searchFeedDatabase).not.toHaveBeenCalled();
    });
});
