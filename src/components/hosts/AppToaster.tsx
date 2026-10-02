import { appToastManagers } from '@/services/toastService';
import { ToastProvider } from '@/ui/shadcn/toast';

import './AppToaster.css';

const TITLE_BAR_VIEWPORT_OFFSET = 'data-[position*=top]:top-[calc(2rem+32px)]';
const DEFAULT_TOAST_TIMEOUT_MS = 4000;

export function AppToaster() {
    return (
        <>
            <ToastProvider
                position="top-center"
                timeout={DEFAULT_TOAST_TIMEOUT_MS}
                toastManager={appToastManagers['top-center']}
                viewportClassName={TITLE_BAR_VIEWPORT_OFFSET}
            />
            <ToastProvider
                position="bottom-right"
                timeout={DEFAULT_TOAST_TIMEOUT_MS}
                toastManager={appToastManagers['bottom-right']}
            />
            <ToastProvider
                position="bottom-center"
                timeout={DEFAULT_TOAST_TIMEOUT_MS}
                toastManager={appToastManagers['bottom-center']}
            />
        </>
    );
}
