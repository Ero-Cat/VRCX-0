import { useState } from 'react';
import { useTranslation } from 'react-i18next';

import { commands } from '@/platform/tauri/bindings';
import type {
    GameLogImportConsent,
    GameLogImportFile,
    GameLogImportFileStatus
} from '@/platform/tauri/bindings';
import { toast } from '@/services/toastService';
import { useRuntimeStore } from '@/state/runtimeStore';
import { Button } from '@/ui/shadcn/button';
import { Checkbox } from '@/ui/shadcn/checkbox';
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle
} from '@/ui/shadcn/dialog';
import { Field, FieldLabel } from '@/ui/shadcn/field';

const keyPrefix = 'view.settings.advanced.advanced_ui.import_recovery.game_log';

const statusKeys: Record<GameLogImportFileStatus, string> = {
    ready: 'status_ready',
    accountUnverified: 'status_account_unverified',
    accountMismatch: 'status_account_mismatch',
    liveFile: 'status_live_file',
    unreadable: 'status_unreadable'
};

const skippedStatusKeys: Record<GameLogImportFileStatus, string> = {
    ...statusKeys,
    accountUnverified: 'status_account_unverified_skipped',
    accountMismatch: 'status_account_mismatch_skipped'
};

type ConsentFieldProps = {
    id: string;
    warning: string;
    label: string;
    checked: boolean;
    disabled: boolean;
    destructive?: boolean;
    onCheckedChange: (checked: boolean) => void;
};

function ConsentField({
    id,
    warning,
    label,
    checked,
    disabled,
    destructive,
    onCheckedChange
}: ConsentFieldProps) {
    return (
        <div className="flex flex-col gap-2 text-sm">
            <p
                className={
                    destructive ? 'text-destructive' : 'text-muted-foreground'
                }
            >
                {warning}
            </p>
            <Field orientation="horizontal">
                <Checkbox
                    id={id}
                    checked={checked}
                    disabled={disabled}
                    onCheckedChange={(value) => onCheckedChange(value === true)}
                />
                <FieldLabel htmlFor={id}>{label}</FieldLabel>
            </Field>
        </div>
    );
}

type GameLogImportDialogProps = {
    files: GameLogImportFile[];
    onOpenChange: (open: boolean) => void;
};

export function GameLogImportDialog({
    files,
    onOpenChange
}: GameLogImportDialogProps) {
    const { t } = useTranslation();
    const setSystemHostOpen = useRuntimeStore(
        (state) => state.setSystemHostOpen
    );
    const [consent, setConsent] = useState<GameLogImportConsent>({
        unverifiedAccount: false,
        accountMismatch: false
    });
    const [running, setRunning] = useState(false);
    const [results, setResults] = useState<GameLogImportFile[] | null>(null);
    const hasStatus = (status: GameLogImportFileStatus) =>
        files.some((file) => file.status === status);
    const importable = files.some(
        (file) =>
            file.status === 'ready' ||
            (consent.unverifiedAccount &&
                file.status === 'accountUnverified') ||
            (consent.accountMismatch && file.status === 'accountMismatch')
    );

    async function runImport() {
        setRunning(true);
        try {
            setResults(
                await commands.appGameLogImport(
                    files.map((file) => file.path),
                    consent
                )
            );
        } catch (error: unknown) {
            toast.add({
                type: 'error',
                title: t(`${keyPrefix}.failed`),
                description:
                    error instanceof Error ? error.message : String(error)
            });
        } finally {
            setRunning(false);
        }
    }

    function openBackup() {
        onOpenChange(false);
        setSystemHostOpen('profileBackupOpen', true);
    }

    function fileLabel(file: GameLogImportFile) {
        if (file.imported) {
            return file.insertedCount > 0
                ? t(`${keyPrefix}.inserted`, { count: file.insertedCount })
                : t(`${keyPrefix}.nothing_new`);
        }
        return t(
            `${keyPrefix}.${(results ? skippedStatusKeys : statusKeys)[file.status]}`
        );
    }

    return (
        <Dialog
            open
            onOpenChange={(open) => {
                if (!running) {
                    onOpenChange(open);
                }
            }}
        >
            <DialogContent className="sm:max-w-lg">
                <DialogHeader>
                    <DialogTitle>{t(`${keyPrefix}.dialog_title`)}</DialogTitle>
                    <DialogDescription>
                        {t(`${keyPrefix}.integrity_notice`)}
                    </DialogDescription>
                </DialogHeader>
                {results ? null : (
                    <div className="text-muted-foreground flex items-center justify-between gap-2 text-sm">
                        <span>{t(`${keyPrefix}.backup_notice`)}</span>
                        <Button
                            type="button"
                            variant="ghost"
                            size="sm"
                            className="text-muted-foreground shrink-0"
                            disabled={running}
                            onClick={openBackup}
                        >
                            {t(`${keyPrefix}.open_backup`)}
                        </Button>
                    </div>
                )}
                <ul className="flex max-h-64 flex-col gap-1 overflow-y-auto text-sm">
                    {(results ?? files).map((file) => (
                        <li
                            key={file.path}
                            className="flex items-center justify-between gap-3"
                        >
                            <span className="min-w-0 truncate font-mono text-xs">
                                {file.fileName}
                            </span>
                            <span className="text-muted-foreground shrink-0 text-xs">
                                {fileLabel(file)}
                            </span>
                        </li>
                    ))}
                </ul>
                {!results && hasStatus('accountUnverified') ? (
                    <ConsentField
                        id="game-log-import-confirm-unverified"
                        warning={t(`${keyPrefix}.unverified_warning`)}
                        label={t(`${keyPrefix}.confirm_unverified`)}
                        checked={consent.unverifiedAccount}
                        disabled={running}
                        onCheckedChange={(unverifiedAccount) =>
                            setConsent((current) => ({
                                ...current,
                                unverifiedAccount
                            }))
                        }
                    />
                ) : null}
                {!results && hasStatus('accountMismatch') ? (
                    <ConsentField
                        id="game-log-import-confirm-mismatch"
                        warning={t(`${keyPrefix}.mismatch_warning`)}
                        label={t(`${keyPrefix}.confirm_mismatch`)}
                        checked={consent.accountMismatch}
                        disabled={running}
                        destructive
                        onCheckedChange={(accountMismatch) =>
                            setConsent((current) => ({
                                ...current,
                                accountMismatch
                            }))
                        }
                    />
                ) : null}
                {results?.some((file) => file.insertedCount > 0) ? (
                    <p className="text-muted-foreground text-sm">
                        {t(`${keyPrefix}.done_hint`)}
                    </p>
                ) : null}
                <DialogFooter>
                    {results ? (
                        <Button
                            type="button"
                            onClick={() => onOpenChange(false)}
                        >
                            {t(`${keyPrefix}.done`)}
                        </Button>
                    ) : (
                        <>
                            <Button
                                type="button"
                                variant="outline"
                                disabled={running}
                                onClick={() => onOpenChange(false)}
                            >
                                {t('confirm.cancel_button')}
                            </Button>
                            <Button
                                type="button"
                                disabled={running || !importable}
                                onClick={() => void runImport()}
                            >
                                {t(`${keyPrefix}.import`)}
                            </Button>
                        </>
                    )}
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
}
