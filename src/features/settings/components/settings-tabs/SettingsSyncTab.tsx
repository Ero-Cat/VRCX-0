import { CloudCogIcon, RefreshCwIcon } from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';
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
    return value.replace('T', ' ').replace(/([+-]\d{2}:\d{2}|Z)$/, '');
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
                    title: t('view.settings.sync.now_done', {
                        pushed: snapshot.lastPushedOps ?? 0,
                        pulled: snapshot.lastPulledOps ?? 0
                    })
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
            <SettingsCard
                cardId="sync-connection"
                title={t('view.settings.sync.connection.title')}
                description={t('view.settings.sync.connection.description')}
            >
                <Field label={t('view.settings.sync.connection.host_label')}>
                    <Input
                        value={host}
                        onChange={(event) => setHost(event.target.value)}
                        placeholder="192.168.1.10"
                        spellCheck={false}
                    />
                </Field>
                <Field label={t('view.settings.sync.connection.port_label')}>
                    <Input
                        value={port}
                        onChange={(event) => setPort(event.target.value)}
                        inputMode="numeric"
                    />
                </Field>
                <Field label={t('view.settings.sync.connection.user_label')}>
                    <Input
                        value={user}
                        onChange={(event) => setUser(event.target.value)}
                        placeholder="vrcx_sync"
                        spellCheck={false}
                        autoComplete="off"
                    />
                </Field>
                <Field
                    label={t('view.settings.sync.connection.password_label')}
                >
                    <Input
                        type="password"
                        value={password}
                        onChange={(event) => setPassword(event.target.value)}
                        placeholder={
                            hasPassword
                                ? t(
                                      'view.settings.sync.connection.password_saved'
                                  )
                                : '••••••••'
                        }
                        autoComplete="new-password"
                    />
                </Field>
                <Field
                    label={t('view.settings.sync.connection.database_label')}
                >
                    <Input
                        value={database}
                        onChange={(event) => setDatabase(event.target.value)}
                        placeholder="vrcx0"
                        spellCheck={false}
                    />
                </Field>
                <Field
                    label={t('view.settings.sync.connection.tls_verify_label')}
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
                    label={t('view.settings.sync.connection.plaintext_label')}
                    description={t(
                        'view.settings.sync.connection.plaintext_hint'
                    )}
                >
                    <Switch
                        checked={allowPlaintext}
                        onCheckedChange={setAllowPlaintext}
                    />
                </Field>
                <div className="flex flex-wrap items-center gap-2 pt-1">
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
                            : t('view.settings.sync.connection.test_button')}
                    </Button>
                    {testResult ? (
                        <span
                            className={
                                testResult.ok
                                    ? 'text-primary text-sm'
                                    : 'text-destructive text-sm'
                            }
                        >
                            {testResult.ok
                                ? t('view.settings.sync.connection.test_ok', {
                                      version: testResult.serverVersion
                                          .split(' ')
                                          .slice(0, 2)
                                          .join(' '),
                                      ms: testResult.latencyMs
                                  })
                                : t(
                                      'view.settings.sync.connection.test_failed',
                                      {
                                          error: testResult.error ?? 'unknown'
                                      }
                                  )}
                        </span>
                    ) : null}
                </div>
                <div className="flex flex-wrap items-center gap-3 pt-2">
                    <Switch
                        checked={enabled}
                        onCheckedChange={(next) => void handleSave(next)}
                        disabled={saving}
                        aria-label={t(
                            'view.settings.sync.connection.enable_label'
                        )}
                    />
                    <span className="text-sm">
                        {t('view.settings.sync.connection.enable_label')}
                    </span>
                </div>
            </SettingsCard>

            <SettingsCard
                cardId="sync-options"
                title={t('view.settings.sync.options.title')}
                description={t('view.settings.sync.options.description')}
            >
                <Field
                    label={t('view.settings.sync.connection.interval_label')}
                    description={t(
                        'view.settings.sync.connection.interval_hint'
                    )}
                >
                    <Input
                        value={intervalSec}
                        onChange={(event) => setIntervalSec(event.target.value)}
                        inputMode="numeric"
                    />
                </Field>
            </SettingsCard>

            <SettingsCard
                cardId="sync-status"
                title={t('view.settings.sync.status.title')}
                description={t('view.settings.sync.status.description')}
            >
                {status ? (
                    <div className="flex flex-col gap-1.5 text-sm">
                        <div className="flex items-center justify-between gap-4">
                            <span className="text-muted-foreground">
                                {t('view.settings.sync.status.phase')}
                            </span>
                            <PhaseBadge phase={status.phase} />
                        </div>
                        <StatusRow
                            label={t('view.settings.sync.status.device_id')}
                            value={status.deviceId}
                        />
                        <StatusRow
                            label={t('view.settings.sync.status.last_cycle')}
                            value={formatTime(
                                status.lastCycleAt ?? status.lastPullAt ?? null
                            )}
                        />
                        <StatusRow
                            label={t('view.settings.sync.status.last_push')}
                            value={formatTime(status.lastPushAt)}
                        />
                        <StatusRow
                            label={t('view.settings.sync.status.last_pull')}
                            value={formatTime(status.lastPullAt)}
                        />
                        <div className="flex items-baseline justify-between gap-4">
                            <span className="text-muted-foreground">
                                {t('view.settings.sync.status.pending')}
                            </span>
                            <span
                                className={
                                    status.pendingOutbox > 0
                                        ? 'font-mono text-xs text-amber-400'
                                        : 'font-mono text-xs'
                                }
                            >
                                {String(status.pendingOutbox)}
                            </span>
                        </div>
                        <StatusRow
                            label={t(
                                'view.settings.sync.status.schema_version'
                            )}
                            value={String(status.remoteSchemaVersion)}
                        />
                        {status.lastError ? (
                            <p className="text-destructive">
                                {status.lastError}
                            </p>
                        ) : null}
                        {bootstrap?.running ? (
                            <BootstrapProgressView progress={bootstrap} />
                        ) : null}
                        <div className="flex items-center gap-2 pt-2">
                            <Button
                                variant="outline"
                                onClick={handleSyncNow}
                                disabled={!status.enabled || syncingNow}
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
                        </div>
                        {status.remoteDevices.length > 0 ? (
                            <div className="pt-2">
                                <p className="mb-1 font-medium">
                                    {t('view.settings.sync.status.devices')}
                                </p>
                                <ul className="flex flex-col gap-0.5">
                                    {status.remoteDevices.map((device) => (
                                        <li
                                            key={device.deviceId}
                                            className="text-muted-foreground"
                                        >
                                            <CloudCogIcon className="mr-1 inline size-3.5" />
                                            {device.deviceId === status.deviceId
                                                ? t(
                                                      'view.settings.sync.status.this_device',
                                                      {
                                                          id: device.deviceId.slice(
                                                              0,
                                                              8
                                                          )
                                                      }
                                                  )
                                                : device.deviceId.slice(0, 8)}
                                            {device.appVersion
                                                ? ` · ${device.appVersion}`
                                                : ''}
                                        </li>
                                    ))}
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
        <div className="mt-2 flex flex-col gap-2 rounded-md border p-3">
            <div className="flex items-center justify-between gap-4">
                <span className="flex items-center gap-2 font-medium">
                    <Spinner className="size-3.5" />
                    {t(`view.settings.sync.phase.${progress.phase}`, {
                        defaultValue: progress.phase
                    })}
                </span>
                <span className="text-muted-foreground text-xs">
                    {t('view.settings.sync.progress.tables_of', {
                        done: progress.tablesDone,
                        total: progress.tablesTotal
                    })}
                </span>
            </div>
            <Progress value={tablesPercent} />
            <div className="flex items-baseline justify-between gap-4">
                <p className="text-muted-foreground truncate font-mono text-xs">
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
                <div className="max-h-44 overflow-y-auto rounded border px-2 py-1">
                    {(progress.tables ?? []).map((table) => (
                        <div
                            key={table.name}
                            className="flex items-baseline justify-between gap-3 py-0.5"
                        >
                            <span className="truncate font-mono text-[11px]">
                                {table.done ? '✓ ' : ''}
                                {table.name}
                            </span>
                            <span className="text-muted-foreground shrink-0 text-[11px]">
                                {table.done
                                    ? t(
                                          'view.settings.sync.progress.table_done',
                                          {
                                              total: table.rowsTotal
                                          }
                                      )
                                    : t(
                                          'view.settings.sync.progress.table_done',
                                          {
                                              total: table.rowsTotal
                                          }
                                      )}
                            </span>
                        </div>
                    ))}
                </div>
            ) : null}
        </div>
    );
}

function StatusRow({ label, value }: { label: string; value: string }) {
    return (
        <div className="flex items-baseline justify-between gap-4">
            <span className="text-muted-foreground">{label}</span>
            <span className="font-mono text-xs">{value}</span>
        </div>
    );
}
