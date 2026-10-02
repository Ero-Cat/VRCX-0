// @vitest-environment jsdom

import {
    cleanup,
    fireEvent,
    render,
    screen,
    waitFor
} from '@testing-library/react';
import type { PropsWithChildren } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type {
    GameLogImportConsent,
    GameLogImportFile,
    GameLogImportFileStatus
} from '@/platform/tauri/bindings';

const mocks = vi.hoisted(() => ({
    select: vi.fn<() => Promise<GameLogImportFile[]>>(),
    importFiles:
        vi.fn<
            (
                paths: string[],
                consent: GameLogImportConsent
            ) => Promise<GameLogImportFile[]>
        >(),
    toast: vi.fn(),
    setSystemHostOpen: vi.fn()
}));

vi.mock('@/platform/tauri/bindings', () => ({
    commands: {
        appGameLogImportSelectFiles: mocks.select,
        appGameLogImport: mocks.importFiles
    }
}));
vi.mock('@/services/toastService', () => ({ toast: { add: mocks.toast } }));
vi.mock('@/state/runtimeStore', () => ({
    useRuntimeStore: (
        selector: (state: {
            setSystemHostOpen: typeof mocks.setSystemHostOpen;
        }) => unknown
    ) => selector({ setSystemHostOpen: mocks.setSystemHostOpen })
}));
vi.mock('@/ui/shadcn/dialog', () => ({
    Dialog: ({ children, open }: PropsWithChildren<{ open: boolean }>) =>
        open ? <>{children}</> : null,
    DialogContent: ({ children }: PropsWithChildren) => (
        <section>{children}</section>
    ),
    DialogDescription: ({ children }: PropsWithChildren) => <p>{children}</p>,
    DialogFooter: ({ children }: PropsWithChildren) => (
        <footer>{children}</footer>
    ),
    DialogHeader: ({ children }: PropsWithChildren) => (
        <header>{children}</header>
    ),
    DialogTitle: ({ children }: PropsWithChildren) => <h1>{children}</h1>
}));
vi.mock('../SettingsField', () => ({
    Field: ({ children }: PropsWithChildren) => <div>{children}</div>
}));

import { GameLogImportField } from './GameLogImportField';

const keyPrefix = 'view.settings.advanced.advanced_ui.import_recovery.game_log';

function file(
    name: string,
    status: GameLogImportFileStatus,
    overrides: Partial<GameLogImportFile> = {}
): GameLogImportFile {
    return {
        path: `C:\\logs\\${name}`,
        fileName: name,
        status,
        imported: false,
        insertedCount: 0,
        ...overrides
    };
}

async function openWith(files: GameLogImportFile[]) {
    mocks.select.mockResolvedValue(files);
    render(<GameLogImportField />);
    fireEvent.click(
        screen.getByRole('button', { name: `${keyPrefix}.select` })
    );
    await screen.findByText(`${keyPrefix}.dialog_title`);
}

function importButton() {
    return screen.getByRole('button', {
        name: `${keyPrefix}.import`
    }) as HTMLButtonElement;
}

afterEach(cleanup);
beforeEach(() => {
    vi.resetAllMocks();
});

describe('GameLogImportField', () => {
    it('does nothing when the file picker is cancelled', async () => {
        mocks.select.mockResolvedValue([]);
        render(<GameLogImportField />);

        fireEvent.click(
            screen.getByRole('button', { name: `${keyPrefix}.select` })
        );

        await waitFor(() => expect(mocks.select).toHaveBeenCalledOnce());
        expect(screen.queryByText(`${keyPrefix}.dialog_title`)).toBeNull();
    });

    it('imports logs without an account line only after the user confirms they are theirs', async () => {
        await openWith([
            file('own.txt', 'ready'),
            file('unknown.txt', 'accountUnverified')
        ]);
        mocks.importFiles.mockResolvedValue([
            file('own.txt', 'ready', { imported: true, insertedCount: 4 }),
            file('unknown.txt', 'accountUnverified', {
                imported: true,
                insertedCount: 0
            })
        ]);

        expect(
            screen.getByText(`${keyPrefix}.unverified_warning`)
        ).toBeTruthy();
        fireEvent.click(
            screen.getByRole('checkbox', {
                name: `${keyPrefix}.confirm_unverified`
            })
        );
        fireEvent.click(importButton());

        await waitFor(() =>
            expect(mocks.importFiles).toHaveBeenCalledWith(
                ['C:\\logs\\own.txt', 'C:\\logs\\unknown.txt'],
                { unverifiedAccount: true, accountMismatch: false }
            )
        );
        expect(await screen.findByText(`${keyPrefix}.inserted`)).toBeTruthy();
        expect(screen.getByText(`${keyPrefix}.nothing_new`)).toBeTruthy();
        expect(screen.getByText(`${keyPrefix}.done_hint`)).toBeTruthy();
        expect(
            screen.getByRole('button', { name: `${keyPrefix}.done` })
        ).toBeTruthy();
    });

    it('imports a log from another account only after the user accepts the consequences', async () => {
        await openWith([file('alt.txt', 'accountMismatch')]);
        mocks.importFiles.mockResolvedValue([
            file('alt.txt', 'accountMismatch', {
                imported: true,
                insertedCount: 3
            })
        ]);

        expect(importButton().disabled).toBe(true);
        expect(screen.getByText(`${keyPrefix}.mismatch_warning`)).toBeTruthy();
        fireEvent.click(
            screen.getByRole('checkbox', {
                name: `${keyPrefix}.confirm_mismatch`
            })
        );
        fireEvent.click(importButton());

        await waitFor(() =>
            expect(mocks.importFiles).toHaveBeenCalledWith(
                ['C:\\logs\\alt.txt'],
                { unverifiedAccount: false, accountMismatch: true }
            )
        );
    });

    it('cannot start an import when nothing selected is importable', async () => {
        await openWith([
            file('other.txt', 'accountMismatch'),
            file('output_log_live.txt', 'liveFile'),
            file('unknown.txt', 'accountUnverified')
        ]);

        expect(importButton().disabled).toBe(true);
        expect(
            screen.getByText(`${keyPrefix}.status_account_mismatch`)
        ).toBeTruthy();
        expect(screen.getByText(`${keyPrefix}.status_live_file`)).toBeTruthy();
    });

    it('offers the backup tools before importing', async () => {
        await openWith([file('own.txt', 'ready')]);

        expect(screen.getByText(`${keyPrefix}.integrity_notice`)).toBeTruthy();
        const backup = screen.getByRole('button', {
            name: `${keyPrefix}.open_backup`
        });
        fireEvent.click(backup);

        expect(mocks.setSystemHostOpen).toHaveBeenCalledWith(
            'profileBackupOpen',
            true
        );
        expect(screen.queryByText(`${keyPrefix}.dialog_title`)).toBeNull();
    });

    it('reports a failed import and keeps the dialog open', async () => {
        await openWith([file('own.txt', 'ready')]);
        mocks.importFiles.mockRejectedValue(new Error('disk full'));

        fireEvent.click(importButton());

        await waitFor(() =>
            expect(mocks.toast).toHaveBeenCalledWith(
                expect.objectContaining({ type: 'error' })
            )
        );
        expect(screen.getByText(`${keyPrefix}.dialog_title`)).toBeTruthy();
        expect(importButton().disabled).toBe(false);
    });
});
