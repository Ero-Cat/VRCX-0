import { useMemo } from 'react';

import {
    normalizeEndpoint,
    normalizeUserId,
    type UserFact
} from '@/domain/users/userFacts';
import {
    useUserFactsStore,
    type UserFactsStoreState
} from '@/state/userFactsStore';

export type KnownUserOption = Partial<UserFact> & {
    id: string;
    endpoint: string;
    name?: string;
};

const KNOWN_USER_OPTION_LIMIT = 500;
const NO_USER_FACTS: UserFactsStoreState['usersByKey'] = {};

export function knownUserName(
    user: Partial<KnownUserOption> | null | undefined
) {
    return user?.displayName || user?.username || user?.name || '';
}

export function useKnownUserOptions({
    enabled,
    endpoint,
    excludeUserId = '',
    query = ''
}: {
    enabled: boolean;
    endpoint: string;
    excludeUserId?: string | null;
    query?: string;
}): KnownUserOption[] {
    const usersByKey = useUserFactsStore((state) =>
        enabled ? state.usersByKey : NO_USER_FACTS
    );
    return useMemo(() => {
        const excludedUserId = normalizeUserId(excludeUserId);
        const normalizedEndpoint = normalizeEndpoint(endpoint);
        const normalizedQuery = query.trim().toLowerCase();
        const usersById = new Map<string, KnownUserOption>();
        for (const user of Object.values(usersByKey)) {
            const userId = normalizeUserId(user?.id);
            if (
                userId &&
                userId !== excludedUserId &&
                !usersById.has(userId) &&
                normalizeEndpoint(user?.endpoint || normalizedEndpoint) ===
                    normalizedEndpoint &&
                (!normalizedQuery ||
                    knownUserName(user)
                        .toLowerCase()
                        .includes(normalizedQuery) ||
                    userId.toLowerCase().includes(normalizedQuery))
            ) {
                usersById.set(userId, user);
            }
        }
        return Array.from(usersById.values())
            .sort((left, right) =>
                (knownUserName(left) || left.id).localeCompare(
                    knownUserName(right) || right.id
                )
            )
            .slice(0, KNOWN_USER_OPTION_LIMIT);
    }, [endpoint, excludeUserId, query, usersByKey]);
}
