import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
    latestUpdaterRelease: {
        title: 'VRCX-0 2.7.0',
        currentVersion: 'Preview 20260621-1530',
        latestVersion: 'v2.7.0-latest',
        canonicalVersion: '2.7.0',
        publishedAt: '2026-06-21T07:00:00Z',
        updaterType: 'manual'
    },
    updateLoop: {
        autoDownloadState: 'idle',
        downloadedVersion: null as string | null,
        downloadProgress: 0,
        downloadedBytes: 0
    }
}));

vi.mock('react-i18next', () => ({
    useTranslation: () => ({
        t: (key: string) =>
            ({
                'nav_menu.update': 'Update',
                'message.vrcx_updater.current_version': 'Current Version',
                'message.vrcx_updater.latest_version': 'Latest Version',
                'message.vrcx_updater.released': 'Released'
            })[key] || key
    })
}));

vi.mock('@/ui/shadcn/button', async () => {
    const React = await import('react');

    return {
        Button: ({
            children,
            variant,
            ...props
        }: React.ComponentProps<'button'> & { variant?: string }) =>
            React.createElement(
                'button',
                { ...props, 'data-variant': variant },
                children
            )
    };
});

vi.mock('@/ui/shadcn/hover-card', async () => {
    const ReactRuntime = await import('react');
    type MockRender =
        | React.ReactNode
        | ((props: object, state: object) => React.ReactNode);
    const renderMockSlot = (
        render: MockRender | undefined,
        children: React.ReactNode
    ) => {
        if (typeof render === 'function') {
            return render({}, {});
        }
        return ReactRuntime.isValidElement(render) ? render : children;
    };

    return {
        HoverCard: ({ children }: React.PropsWithChildren) =>
            ReactRuntime.createElement('div', null, children),
        HoverCardContent: ({ children }: React.PropsWithChildren) =>
            ReactRuntime.createElement('div', null, children),
        HoverCardTrigger: ({
            children,
            render
        }: {
            children?: React.ReactNode;
            render?: MockRender;
        }) =>
            ReactRuntime.createElement(
                ReactRuntime.Fragment,
                null,
                renderMockSlot(render, children)
            )
    };
});

vi.mock('@/state/runtimeStore', () => ({
    useRuntimeStore: <T,>(
        selector: (state: {
            updateLoop: typeof mocks.updateLoop & {
                latestUpdaterRelease: typeof mocks.latestUpdaterRelease;
            };
        }) => T
    ) =>
        selector({
            updateLoop: {
                latestUpdaterRelease: mocks.latestUpdaterRelease,
                ...mocks.updateLoop
            }
        })
}));

import { TitleBarUpdateButton } from './TitleBarUpdateButton';

describe('TitleBarUpdateButton', () => {
    beforeEach(() => {
        mocks.updateLoop.autoDownloadState = 'idle';
        mocks.updateLoop.downloadedVersion = null;
        mocks.updateLoop.downloadProgress = 0;
        mocks.updateLoop.downloadedBytes = 0;
    });

    it('renders the update entry with the latest release snapshot', () => {
        const html = renderToStaticMarkup(
            React.createElement(TitleBarUpdateButton, {
                onClick: vi.fn()
            })
        );

        expect(html).toContain('Update');
        expect(html).toContain('VRCX-0 2.7.0');
        expect(html).toContain('Preview 20260621-1530');
        expect(html).toContain('v2.7.0-latest');
    });

    it('shows the downloaded size and progress while downloading the matching update', () => {
        mocks.updateLoop.autoDownloadState = 'downloading';
        mocks.updateLoop.downloadedVersion = '2.7.0';
        mocks.updateLoop.downloadProgress = 42;
        mocks.updateLoop.downloadedBytes = 12_500_000;

        const html = renderToStaticMarkup(
            React.createElement(TitleBarUpdateButton, {
                onClick: vi.fn()
            })
        );

        expect(html).toContain('Update');
        expect(html).toContain('12 MB');
        expect(html).toContain('42%');
    });
});
