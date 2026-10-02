import { describe, expect, it } from 'vitest';

import {
    buildFriendsLocationsSegmentOptions,
    parseConfigArray
} from './friendsLocationsConfig';
import { sanitizeFriendsLocationsDensity } from './friendsLocationsDensity';

describe('friends locations config helpers', () => {
    it('adds current counts without changing segment order', () => {
        expect(
            buildFriendsLocationsSegmentOptions({
                online: 12,
                favorite: 3,
                'same-instance': 2,
                active: 4,
                offline: 80
            }).map(({ value, count }) => [value, count])
        ).toEqual([
            ['online', 12],
            ['favorite', 3],
            ['same-instance', 2],
            ['active', 4],
            ['offline', 80]
        ]);
    });

    it('parses JSON config arrays and drops empty entries', () => {
        expect(parseConfigArray('bad json')).toEqual([]);
        expect(parseConfigArray('["group_a","",null,"group_b"]')).toEqual([
            'group_a',
            'group_b'
        ]);
        expect(parseConfigArray(['group_a', '', 'group_b'])).toEqual([
            'group_a',
            'group_b'
        ]);
    });

    it('falls back to compact density for unknown values', () => {
        expect(sanitizeFriendsLocationsDensity('standard')).toBe('standard');
        expect(sanitizeFriendsLocationsDensity('bad-value')).toBe('compact');
    });
});
