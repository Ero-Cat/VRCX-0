import { useCallback } from 'react';
import { useTranslation } from 'react-i18next';

import { notificationTypeLabelKey } from './notificationViewModel';

export function useNotificationTypeLabel() {
    const { t } = useTranslation();

    return useCallback(
        (type: string | undefined) => {
            const fallback = type || 'unknown';
            const key = notificationTypeLabelKey(fallback);
            const label = String(t(key));
            return label && label !== key ? label : fallback;
        },
        [t]
    );
}
