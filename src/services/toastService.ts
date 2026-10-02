import { Toast } from '@base-ui/react/toast';

import { userFacingErrorMessage } from '@/lib/errorDisplay';
import { links } from '@/shared/constants/link';
import type { AppToastData, AppToastRenderData } from '@/shared/toast';

import i18n from './i18nService';

export const appToastManagers = {
    'top-center': Toast.createToastManager<AppToastRenderData>(),
    'bottom-right': Toast.createToastManager<AppToastRenderData>(),
    'bottom-center': Toast.createToastManager<AppToastRenderData>()
};

type ManagerAddOptions = Parameters<
    (typeof appToastManagers)['top-center']['add']
>[0];

export type AppToastOptions = Omit<ManagerAddOptions, 'data'> & {
    data?: AppToastData;
    position?: keyof typeof appToastManagers;
};

const VRCHAT_STATUS_HOST = new URL(links.vrchatStatus).hostname.toLowerCase();
const URL_PATTERN = /\bhttps?:\/\/[^\s"'<>]+/gi;
const VRCHAT_API_UNAVAILABLE_TIMEOUT_MS = 6000;

function isVrchatApiUnavailableMessage(message: unknown): boolean {
    if (typeof message !== 'string') {
        return false;
    }
    if (message.includes('VRChat API services are currently unavailable')) {
        return true;
    }
    return (message.match(URL_PATTERN) || []).some((url) => {
        try {
            return new URL(url).hostname.toLowerCase() === VRCHAT_STATUS_HOST;
        } catch {
            return false;
        }
    });
}

function add({ position = 'top-center', ...options }: AppToastOptions): string {
    const manager = appToastManagers[position];
    const title =
        options.type === 'error' && typeof options.title === 'string'
            ? userFacingErrorMessage(
                  options.title,
                  i18n.t('common.error.action_failed')
              )
            : options.title;
    let timeout = options.timeout;
    if (timeout === undefined) {
        if (options.type === 'loading') {
            timeout = 0;
        } else if (
            options.type === 'error' &&
            isVrchatApiUnavailableMessage(title)
        ) {
            timeout = VRCHAT_API_UNAVAILABLE_TIMEOUT_MS;
        }
    }
    const actionProps = options.actionProps;
    const id = manager.add({
        ...options,
        title,
        timeout,
        actionProps: actionProps
            ? {
                  ...actionProps,
                  onClick(event) {
                      actionProps.onClick?.(event);
                      if (!event.defaultPrevented) {
                          manager.close(id);
                      }
                  }
              }
            : undefined,
        data: {
            ...options.data,
            closeLabel: i18n.t('common.actions.close')
        }
    });
    return id;
}

function close(id?: string): void {
    for (const manager of Object.values(appToastManagers)) {
        manager.close(id);
    }
}

export const toast = { add, close };
