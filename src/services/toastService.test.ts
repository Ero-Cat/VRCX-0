import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { appToastManagers, toast } from './toastService';

vi.mock('./i18nService', () => ({
    default: { t: (key: string) => key }
}));

beforeEach(() => {
    for (const manager of Object.values(appToastManagers)) {
        vi.spyOn(manager, 'add').mockReturnValue('toast-id');
        vi.spyOn(manager, 'close').mockImplementation(() => undefined);
    }
});

afterEach(() => vi.restoreAllMocks());

describe('application toast policy', () => {
    it.each(['top-center', 'bottom-right', 'bottom-center'] as const)(
        'routes a notification to only the %s viewport',
        (position) => {
            toast.add({ position, title: 'Saved', type: 'success' });
            for (const [key, manager] of Object.entries(appToastManagers)) {
                expect(manager.add).toHaveBeenCalledTimes(
                    key === position ? 1 : 0
                );
            }
        }
    );

    it('uses the provider timeout by default and keeps loading notifications persistent', () => {
        toast.add({ title: 'Saved', type: 'success' });
        expect(appToastManagers['top-center'].add).toHaveBeenLastCalledWith(
            expect.objectContaining({ timeout: undefined })
        );
        toast.add({ title: 'Exporting', type: 'loading' });
        expect(appToastManagers['top-center'].add).toHaveBeenLastCalledWith(
            expect.objectContaining({ timeout: 0 })
        );
        toast.add({ title: 'Retrying', type: 'loading', timeout: 1500 });
        expect(appToastManagers['top-center'].add).toHaveBeenLastCalledWith(
            expect.objectContaining({ timeout: 1500 })
        );
    });

    it('keeps service outage errors visible for 12 seconds unless explicitly configured', () => {
        const title = 'VRChat API services are currently unavailable';
        toast.add({ title, type: 'error' });
        expect(appToastManagers['top-center'].add).toHaveBeenLastCalledWith(
            expect.objectContaining({ title, timeout: 6000 })
        );
        toast.add({ title, type: 'error', timeout: 0 });
        expect(appToastManagers['top-center'].add).toHaveBeenLastCalledWith(
            expect.objectContaining({ timeout: 0 })
        );
    });

    it('matches only the official status host when extending error duration', () => {
        toast.add({ title: 'See https://status.vrchat.com/', type: 'error' });
        expect(appToastManagers['top-center'].add).toHaveBeenLastCalledWith(
            expect.objectContaining({ timeout: 6000 })
        );
        toast.add({
            title: 'See https://status.vrchat.com.example.org/',
            type: 'error'
        });
        expect(appToastManagers['top-center'].add).toHaveBeenLastCalledWith(
            expect.objectContaining({ timeout: undefined })
        );
    });

    it('closes a stable notification in every viewport', () => {
        toast.close('update');
        for (const manager of Object.values(appToastManagers)) {
            expect(manager.close).toHaveBeenCalledWith('update');
        }
    });
});
