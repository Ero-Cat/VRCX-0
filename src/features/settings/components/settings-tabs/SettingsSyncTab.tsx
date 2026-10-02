import {
    ArrowDownIcon,
    ArrowUpIcon,
    CheckCircle2Icon,
    CloudCogIcon,
    DatabaseIcon,
    LaptopIcon,
    RefreshCwIcon,
    ShieldCheckIcon,
    XCircleIcon
} from 'lucide-react';
import {
    cloneElement,
    isValidElement,
    useCallback,
    useEffect,
    useId,
    useRef,
    useState,
    type ReactNode
} from 'react';
import { useTranslation } from 'react-i18next';

import type {
    SyncBootstrapProgress,
    SyncConnectionTestResult,
    SyncStatusSnapshot
} from '@/platform/tauri/bindings';
import {
    configureSync,
    fetchSyncBootstrapProgress,
    fetchSyncConnection,
    fetchSyncStatus,
    testSyncConnection,
    triggerSyncNow
} from '@/repositories/syncRepository';
import { toast } from '@/services/toastService';
import { Badge } from '@/ui/shadcn/badge';
import { Button } from '@/ui/shadcn/button';
import { FieldLabel } from '@/ui/shadcn/field';
import { Input } from '@/ui/shadcn/input';
import { Progress } from '@/ui/shadcn/progress';
import { Spinner } from '@/ui/shadcn/spinner';
import { Switch } from '@/ui/shadcn/switch';

import { SettingsCard } from '../SettingsCard';
import { Field } from '../SettingsField';
import { SettingsTabContent } from '../SettingsViewParts';

const STATUS_POLL_MS = 8000;
const BOOTSTRAP_POLL_MS = 2000;

function formatTime(value: string | null | undefined): string {
    if (!value) {
        return '—';
    }
    // Engine timestamps are UTC ISO strings; render them in the user's
    // timezone so "last sync" matches the wall clock they are watching.
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) {
        return value;
    }
    return date.toLocaleString(undefined, {
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
        hour12: false
    });
}

export function SettingsSyncTab() {
    const { t } = useTranslation();
    const [status, setStatus] = useState<SyncStatusSnapshot | null>(null);
    const [host, setHost] = useState('');
    const [port, setPort] = useState('5432');
    const [user, setUser] = useState('');
    const [password, setPassword] = useState('');
    const [database, setDatabase] = useState('');
    const [hasPassword, setHasPassword] = useState(false);
    const [tlsVerify, setTlsVerify] = useState(false);
    const [allowPlaintext, setAllowPlaintext] = useState(true);
    const [intervalSec, setIntervalSec] = useState('60');
    const [enabled, setEnabled] = useState(false);
    const [testing, setTesting] = useState(false);
    const [testResult, setTestResult] =
        useState<SyncConnectionTestResult | null>(null);
    const [saving, setSaving] = useState(false);
    const [syncingNow, setSyncingNow] = useState(false);
    const [bootstrap, setBootstrap] = useState<SyncBootstrapProgress | null>(
        null
    );

    const refreshStatus = useCallback(async () => {
        try {
            const snapshot = await fetchSyncStatus();
            setStatus(snapshot);
            // The enable switch mirrors the persisted state, not local state.
            setEnabled(snapshot.enabled);
        } catch {
            // Status polling is best-effort; the card shows the last snapshot.
        }
    }, []);

    const mounted = useRef(true);
    useEffect(() => {
        mounted.current = true;
        void refreshStatus();
        void fetchSyncConnection()
            .then((fields) => {
                if (!mounted.current) {
                    return;
                }
                setHost(fields.host);
                setPort(String(fields.port > 0 ? fields.port : 5432));
                setUser(fields.user);
                setDatabase(fields.database);
                setTlsVerify(fields.tlsVerify);
                setAllowPlaintext(fields.allowPlaintext);
                setHasPassword(fields.hasPassword);
                if ((fields.intervalSeconds ?? 0) > 0) {
                    setIntervalSec(String(fields.intervalSeconds));
                }
            })
            .catch(() => undefined);
        return () => {
            mounted.current = false;
        };
    }, [refreshStatus]);

    // Bootstrap progress polls fast while an initial sync is running, slow
    // otherwise; a running bootstrap also refreshes the status card.
    useEffect(() => {
        let cancelled = false;
        const tick = async () => {
            try {
                const progress = await fetchSyncBootstrapProgress();
                if (cancelled) {
                    return;
                }
                setBootstrap(progress);
                if (progress.running) {
                    void refreshStatus();
                }
            } catch {
                // Best-effort polling.
            }
        };
        void tick();
        const interval = window.setInterval(
            () => void tick(),
            bootstrap?.running ? BOOTSTRAP_POLL_MS : STATUS_POLL_MS
        );
        return () => {
            cancelled = true;
            window.clearInterval(interval);
        };
    }, [bootstrap?.running, refreshStatus]);

    const parsePort = () => {
        const parsed = Number.parseInt(port, 10);
        return Number.isFinite(parsed) && parsed > 0 && parsed < 65536
            ? parsed
            : 5432;
    };

    const handleTest = async () => {
        setTesting(true);
        setTestResult(null);
        try {
            setTestResult(
                await testSyncConnection({
                    host,
                    port: parsePort(),
                    user,
                    // Fall back to the stored password when the field is untouched.
                    password: password.length > 0 ? password : null,
                    database,
                    tlsVerify,
                    allowPlaintext
                })
            );
        } catch (error) {
            setTestResult({
                ok: false,
                serverVersion: '',
                latencyMs: 0,
                error: String(error)
            });
        } finally {
            setTesting(false);
        }
    };

    const handleSave = async (nextEnabled: boolean) => {
        setSaving(true);
        try {
            const snapshot = await configureSync({
                enabled: nextEnabled,
                connection: {
                    host: host.trim(),
                    port: parsePort(),
                    user: user.trim(),
                    password: password.length > 0 ? password : null,
                    database: database.trim(),
                    tlsVerify,
                    allowPlaintext
                },
                intervalSec: Number.parseInt(intervalSec, 10) || undefined
            });
            setStatus(snapshot);
            setEnabled(snapshot.enabled);
            if (password.length > 0) {
                setPassword('');
                setHasPassword(true);
            }
            toast.add({
                type: 'success',
                title: nextEnabled
                    ? t('view.settings.sync.enable_toast')
                    : t('view.settings.sync.disable_toast')
            });
        } catch (error) {
            toast.add({ type: 'error', title: String(error) });
        } finally {
            setSaving(false);
        }
    };

    const handleSyncNow = async () => {
        setSyncingNow(true);
        try {
            const snapshot = await triggerSyncNow();
            setStatus(snapshot);
            if (snapshot.pendingOutbox > 0) {
                toast.add({
                    type: 'success',
                    title: t('view.settings.sync.now_done_pending', {
                        count: snapshot.pendingOutbox
                    })
                });
            } else {
                toast.add({
                    type: 'success',
                    title: t('view.settings.sync.now_done')
                });
            }
        } catch (error) {
            toast.add({ type: 'error', title: String(error) });
        } finally {
            setSyncingNow(false);
        }
    };

    const canTest = host.trim().length > 0 && database.trim().length > 0;

    return (
        <SettingsTabContent value="sync">
            {!enabled ? (
                <div className="text-muted-foreground flex items-start gap-2.5 rounded-md border border-dashed px-4 py-3 text-sm">
                    <ShieldCheckIcon className="mt-0.5 size-4 shrink-0" />
                    <span>
                        {t('view.settings.sync.connection.privacy_note')}
                    </span>
                </div>
            ) : null}

            <SettingsCard
                cardId="sync-connection"
                title={t('view.settings.sync.connection.title')}
                description={t('view.settings.sync.connection.description')}
            >
                <div className="grid grid-cols-2 gap-x-4 gap-y-4 pt-1.5 sm:grid-cols-3">
                    <FormField
                        label={t('view.settings.sync.connection.host_label')}
                    >
                        <Input
                            value={host}
                            onChange={(event) => setHost(event.target.value)}
                            placeholder="192.168.1.10"
                            spellCheck={false}
                        />
                    </FormField>
                    <FormField
                        label={t('view.settings.sync.connection.port_label')}
                    >
                        <Input
                            value={port}
                            onChange={(event) => setPort(event.target.value)}
                            placeholder="5432"
                            inputMode="numeric"
                        />
                    </FormField>
                    <FormField
                        label={t(
                            'view.settings.sync.connection.database_label'
                        )}
                    >
                        <Input
                            value={database}
                            onChange={(event) =>
                                setDatabase(event.target.value)
                            }
                            placeholder="vrcx0"
                            spellCheck={false}
                        />
                    </FormField>
                    <FormField
                        label={t('view.settings.sync.connection.user_label')}
                    >
                        <Input
                            value={user}
                            onChange={(event) => setUser(event.target.value)}
                            placeholder="vrcx_sync"
                            spellCheck={false}
                            autoComplete="off"
                        />
                    </FormField>
                    <FormField
                        label={t(
                            'view.settings.sync.connection.password_label'
                        )}
                    >
                        <Input
                            type="password"
                            value={password}
                            onChange={(event) =>
                                setPassword(event.target.value)
                            }
                            placeholder="••••••••"
                            autoComplete="new-password"
                        />
                        {hasPassword ? (
                            <p className="text-muted-foreground text-xs">
                                {t(
                                    'view.settings.sync.connection.password_saved'
                                )}
                            </p>
                        ) : null}
                    </FormField>
                </div>

                <div className="border-stroke-subtle mt-5 border-t">
                    <Field
                        label={t(
                            'view.settings.sync.connection.tls_verify_label'
                        )}
                        description={t(
                            'view.settings.sync.connection.tls_verify_hint'
                        )}
                    >
                        <Switch
                            checked={tlsVerify}
                            onCheckedChange={setTlsVerify}
                        />
                    </Field>
                    <Field
                        label={t(
                            'view.settings.sync.connection.plaintext_label'
                        )}
                        description={t(
                            'view.settings.sync.connection.plaintext_hint'
                        )}
                    >
                        <Switch
                            checked={allowPlaintext}
                            onCheckedChange={setAllowPlaintext}
                        />
                    </Field>
                    <Field
                        label={t(
                            'view.settings.sync.connection.interval_label'
                        )}
                        description={t(
                            'view.settings.sync.connection.interval_hint'
                        )}
                    >
                        <div className="flex items-center justify-end gap-2">
                            <Input
                                value={intervalSec}
                                onChange={(event) =>
                                    setIntervalSec(event.target.value)
                                }
                                inputMode="numeric"
                                className="w-20"
                            />
                            <span className="text-muted-foreground text-sm">
                                {t(
                                    'view.settings.sync.connection.interval_unit'
                                )}
                            </span>
                        </div>
                    </Field>
                </div>

                <div className="border-stroke-subtle mt-1 flex flex-wrap items-center justify-between gap-x-4 gap-y-3 border-t pt-4">
                    <div className="flex min-w-0 flex-wrap items-center gap-2.5">
                        <Button
                            variant="outline"
                            onClick={handleTest}
                            disabled={testing || !canTest}
                        >
                            {testing ? (
                                <Spinner className="size-4" />
                            ) : (
                                <RefreshCwIcon />
                            )}
                            {testing
                                ? t('view.settings.sync.connection.testing')
                                : t(
                                      'view.settings.sync.connection.test_button'
                                  )}
                        </Button>
                        {testResult ? (
                            <TestResultBadge result={testResult} />
                        ) : null}
                    </div>
                    <div className="flex shrink-0 items-center gap-2.5">
                        <span className="text-sm font-medium">
                            {t('view.settings.sync.connection.enable_label')}
                        </span>
                        <Switch
                            checked={enabled}
                            onCheckedChange={(next) => void handleSave(next)}
                            disabled={saving}
                            aria-label={t(
                                'view.settings.sync.connection.enable_label'
                            )}
                        />
                    </div>
                </div>
            </SettingsCard>

            <SettingsCard
                cardId="sync-status"
                title={t('view.settings.sync.status.title')}
                description={t('view.settings.sync.status.description')}
                action={
                    <Button
                        variant="outline"
                        size="sm"
                        onClick={handleSyncNow}
                        disabled={!status?.enabled || syncingNow}
                    >
                        {syncingNow ? (
                            <Spinner className="size-4" />
                        ) : (
                            <RefreshCwIcon />
                        )}
                        {syncingNow
                            ? t('view.settings.sync.now_running')
                            : t('view.settings.sync.status.sync_now')}
                    </Button>
                }
            >
                {status ? (
                    <div className="flex flex-col gap-4 pt-1">
                        <div className="flex flex-wrap items-center gap-x-5 gap-y-2">
                            <PhaseBadge phase={status.phase} />
                            {status.pendingOutbox > 0 ? (
                                <span className="flex items-center gap-1.5 text-xs text-amber-500">
                                    <DatabaseIcon className="size-3.5" />
                                    {t('view.settings.sync.status.pending', {
                                        count: status.pendingOutbox
                                    })}
                                </span>
                            ) : null}
                        </div>

                        <div className="grid grid-cols-1 gap-x-8 gap-y-2 sm:grid-cols-2">
                            <StatusCell
                                label={t(
                                    'view.settings.sync.status.last_cycle'
                                )}
                                value={formatTime(
                                    status.lastCycleAt ??
                                        status.lastPullAt ??
                                        null
                                )}
                            />
                            <StatusCell
                                label={t('view.settings.sync.status.device_id')}
                                value={status.deviceId.slice(0, 8)}
                            />
                            <StatusCell
                                label={t(
                                    'view.settings.sync.status.last_pushed'
                                )}
                                value={t(
                                    'view.settings.sync.status.ops_count',
                                    {
                                        count: status.lastPushedOps ?? 0
                                    }
                                )}
                            />
                            <StatusCell
                                label={t(
                                    'view.settings.sync.status.last_pulled'
                                )}
                                value={t(
                                    'view.settings.sync.status.ops_count',
                                    {
                                        count: status.lastPulledOps ?? 0
                                    }
                                )}
                            />
                        </div>

                        {status.enabled ? (
                            <div>
                                <p className="mb-2 flex items-center gap-1.5 text-xs font-medium">
                                    <DatabaseIcon className="size-3.5" />
                                    {t(
                                        'view.settings.sync.status.cycle_tables'
                                    )}
                                </p>
                                {(status.lastCycleTables?.length ?? 0) > 0 ? (
                                    <div className="max-h-52 overflow-y-auto rounded-md border px-3 py-1.5">
                                        {(status.lastCycleTables ?? []).map(
                                            (row) => (
                                                <div
                                                    key={row.table}
                                                    className="flex items-center justify-between gap-3 py-1"
                                                >
                                                    <span className="min-w-0 truncate font-mono text-[11px]">
                                                        {row.table}
                                                    </span>
                                                    <span className="flex shrink-0 items-center gap-2.5 text-[11px]">
                                                        {row.pushed > 0 ? (
                                                            <span
                                                                className="text-primary flex items-center gap-0.5"
                                                                title={t(
                                                                    'view.settings.sync.status.pushed_tip',
                                                                    {
                                                                        count: row.pushed
                                                                    }
                                                                )}
                                                            >
                                                                <ArrowUpIcon className="size-3" />
                                                                {row.pushed}
                                                            </span>
                                                        ) : null}
                                                        {row.pulled > 0 ? (
                                                            <span
                                                                className="text-muted-foreground flex items-center gap-0.5"
                                                                title={t(
                                                                    'view.settings.sync.status.pulled_tip',
                                                                    {
                                                                        count: row.pulled
                                                                    }
                                                                )}
                                                            >
                                                                <ArrowDownIcon className="size-3" />
                                                                {row.pulled}
                                                            </span>
                                                        ) : null}
                                                    </span>
                                                </div>
                                            )
                                        )}
                                    </div>
                                ) : (
                                    <p className="text-muted-foreground text-xs">
                                        {t(
                                            'view.settings.sync.status.cycle_tables_empty'
                                        )}
                                    </p>
                                )}
                            </div>
                        ) : null}

                        {status.lastError ? (
                            <div className="text-destructive border-destructive/30 bg-destructive/5 flex items-start gap-2 rounded-md border px-3 py-2 text-xs">
                                <XCircleIcon className="mt-0.5 size-3.5 shrink-0" />
                                <span className="break-all">
                                    {status.lastError}
                                </span>
                            </div>
                        ) : null}

                        {bootstrap?.running ? (
                            <BootstrapProgressView progress={bootstrap} />
                        ) : null}

                        {status.remoteDevices.length > 0 ? (
                            <div>
                                <p className="mb-2 flex items-center gap-1.5 text-xs font-medium">
                                    <LaptopIcon className="size-3.5" />
                                    {t('view.settings.sync.status.devices')}
                                </p>
                                <ul className="flex flex-col gap-1.5">
                                    {status.remoteDevices.map((device) => {
                                        const isSelf =
                                            device.deviceId === status.deviceId;
                                        return (
                                            <li
                                                key={device.deviceId}
                                                className="flex items-center justify-between gap-3 rounded-md border px-3 py-2 text-xs"
                                            >
                                                <span className="flex min-w-0 items-center gap-1.5">
                                                    {isSelf ? (
                                                        <CloudCogIcon className="text-primary size-3.5 shrink-0" />
                                                    ) : (
                                                        <LaptopIcon className="text-muted-foreground size-3.5 shrink-0" />
                                                    )}
                                                    <span className="truncate font-mono">
                                                        {device.deviceId.slice(
                                                            0,
                                                            8
                                                        )}
                                                    </span>
                                                    {isSelf ? (
                                                        <Badge
                                                            variant="secondary"
                                                            className="h-4 px-1.5 text-[10px]"
                                                        >
                                                            {t(
                                                                'view.settings.sync.status.this_device_tag'
                                                            )}
                                                        </Badge>
                                                    ) : null}
                                                </span>
                                                <span className="text-muted-foreground shrink-0">
                                                    {device.appVersion || '—'}
                                                </span>
                                            </li>
                                        );
                                    })}
                                </ul>
                            </div>
                        ) : null}
                    </div>
                ) : (
                    <p className="text-muted-foreground text-sm">
                        {t('view.settings.sync.status.unavailable')}
                    </p>
                )}
            </SettingsCard>
        </SettingsTabContent>
    );
}

function FormField({
    label,
    children
}: {
    label: string;
    children: ReactNode;
}) {
    const id = useId();
    const control = isValidElement<{ id?: string }>(children)
        ? cloneElement(children, { id })
        : children;
    return (
        <div className="flex min-w-0 flex-col gap-2">
            <FieldLabel
                htmlFor={id}
                className="text-sm leading-none font-medium"
            >
                {label}
            </FieldLabel>
            {control}
        </div>
    );
}

function TestResultBadge({ result }: { result: SyncConnectionTestResult }) {
    const { t } = useTranslation();
    if (result.ok) {
        return (
            <span className="text-primary flex items-center gap-1.5 text-sm">
                <CheckCircle2Icon className="size-4 shrink-0" />
                {t('view.settings.sync.connection.test_ok', {
                    ms: result.latencyMs
                })}
            </span>
        );
    }
    return (
        <span
            className="text-destructive flex min-w-0 items-start gap-1.5 text-sm"
            title={result.error ?? undefined}
        >
            <XCircleIcon className="mt-0.5 size-4 shrink-0" />
            <span className="truncate">
                {t('view.settings.sync.connection.test_failed_short')}
            </span>
        </span>
    );
}

function PhaseBadge({ phase }: { phase: string }) {
    const { t } = useTranslation();
    const label = t(`view.settings.sync.phase.${phase}`, {
        defaultValue: phase
    });
    const tone =
        phase === 'error'
            ? 'bg-destructive/15 text-destructive'
            : ['bootstrap', 'reconciling', 'running', 'push', 'merge'].includes(
                    phase
                )
              ? 'bg-primary/15 text-primary'
              : 'bg-muted text-muted-foreground';
    return (
        <Badge className={tone} variant="secondary">
            {phase === 'idle' ? (
                <CheckCircle2Icon className="mr-1 size-3" />
            ) : null}
            {label}
        </Badge>
    );
}

function BootstrapProgressView({
    progress
}: {
    progress: SyncBootstrapProgress;
}) {
    const { t } = useTranslation();
    const tablesPercent =
        progress.tablesTotal > 0
            ? Math.round((progress.tablesDone / progress.tablesTotal) * 100)
            : 0;
    const rowsTotal = progress.rowsTotal ?? 0;
    const rowsPercent =
        rowsTotal > 0
            ? Math.min(100, Math.round((progress.rowsDone / rowsTotal) * 100))
            : null;
    const tableRowsTotal = progress.currentTableRowsTotal ?? 0;
    const currentTableRowsDone = progress.currentTableRowsDone ?? 0;
    const tableRowsPercent =
        tableRowsTotal > 0
            ? Math.min(
                  100,
                  Math.round((currentTableRowsDone / tableRowsTotal) * 100)
              )
            : null;
    return (
        <div className="flex flex-col gap-3 rounded-md border p-4">
            <div className="flex items-center justify-between gap-4">
                <span className="flex items-center gap-2 text-sm font-medium">
                    <Spinner className="size-3.5" />
                    {t(`view.settings.sync.phase.${progress.phase}`, {
                        defaultValue: progress.phase
                    })}
                </span>
                <span className="text-muted-foreground shrink-0 text-xs">
                    {t('view.settings.sync.progress.tables_of', {
                        done: progress.tablesDone,
                        total: progress.tablesTotal,
                        percent: tablesPercent
                    })}
                </span>
            </div>
            <Progress value={tablesPercent} />
            <div className="flex items-baseline justify-between gap-4">
                <p className="text-muted-foreground min-w-0 truncate font-mono text-xs">
                    {progress.currentTable}
                </p>
                <span className="text-muted-foreground shrink-0 text-xs">
                    {tableRowsPercent === null
                        ? t('view.settings.sync.progress.rows', {
                              count: currentTableRowsDone
                          })
                        : t('view.settings.sync.progress.table_rows_of', {
                              done: currentTableRowsDone,
                              total: tableRowsTotal,
                              percent: tableRowsPercent
                          })}
                </span>
            </div>
            {tableRowsPercent === null ? null : (
                <Progress value={tableRowsPercent} />
            )}
            <p className="text-muted-foreground text-xs">
                {rowsPercent === null
                    ? t('view.settings.sync.progress.rows', {
                          count: progress.rowsDone
                      })
                    : t('view.settings.sync.progress.rows_of', {
                          done: progress.rowsDone,
                          total: rowsTotal,
                          percent: rowsPercent
                      })}
            </p>
            {(progress.tables?.length ?? 0) > 0 ? (
                <div className="max-h-44 overflow-y-auto rounded-md border px-3 py-1.5">
                    {(progress.tables ?? []).map((table) => (
                        <div
                            key={table.name}
                            className="flex items-baseline justify-between gap-3 py-1"
                        >
                            <span className="flex min-w-0 items-baseline gap-1.5">
                                {table.done ? (
                                    <CheckCircle2Icon className="text-primary mt-0.5 inline size-3 shrink-0 self-center" />
                                ) : null}
                                <span className="truncate font-mono text-[11px]">
                                    {table.name}
                                </span>
                            </span>
                            <span className="text-muted-foreground shrink-0 text-[11px]">
                                {t('view.settings.sync.progress.table_rows', {
                                    done: table.done
                                        ? (table.rowsTotal ?? 0)
                                        : (table.rowsDone ?? 0),
                                    total: table.rowsTotal ?? 0
                                })}
                            </span>
                        </div>
                    ))}
                </div>
            ) : null}
        </div>
    );
}

function StatusCell({
    label,
    value,
    icon
}: {
    label: string;
    value: string;
    icon?: ReactNode;
}) {
    return (
        <div className="flex items-baseline justify-between gap-4">
            <span className="text-muted-foreground flex items-center gap-1.5 text-sm">
                {icon}
                {label}
            </span>
            <span className="font-mono text-xs">{value}</span>
        </div>
    );
}
