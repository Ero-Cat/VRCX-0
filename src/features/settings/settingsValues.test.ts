import { describe, expect, it } from 'vitest';

import { resolveActiveSettingsTab, settingsTabs } from './settingsOptions';
import {
    buildTablePageSizeOptions,
    composeCustomFontFamily,
    createEffectiveCustomFontDraft,
    createCustomFontDraftFromPrefs,
    filterTablePageSizeOptions,
    isValidFontFamilyList,
    normalizeTablePageSizes,
    parseIntegerInput,
    parseWebJson,
    quoteCssFontFamilyName,
    TABLE_PAGE_SIZE_DEFAULTS
} from './settingsValues';

describe('settingsValues', () => {
    it('places AI settings before integrations', () => {
        const tabValues = settingsTabs.map(([value]) => value);
        expect(tabValues.indexOf('ai')).toBeGreaterThanOrEqual(0);
        expect(tabValues.indexOf('ai')).toBeLessThan(
            tabValues.indexOf('integrations')
        );
    });

    it('restores the last settings tab while preserving explicit tab links', () => {
        expect(resolveActiveSettingsTab('', 'social')).toBe('social');
        expect(resolveActiveSettingsTab('media', 'social')).toBe('media');
    });

    it('normalizes table page sizes to the sorted usable choices users can save', () => {
        expect(
            normalizeTablePageSizes(['50', 10, '10', 0, -5, 2000, 'bad', 25])
        ).toEqual([10, 25, 50]);
        expect(normalizeTablePageSizes(['bad', 0])).toEqual(
            TABLE_PAGE_SIZE_DEFAULTS
        );
    });

    it('builds table page size suggestions from defaults and the current draft', () => {
        const options = buildTablePageSizeOptions([12, 50, '75']);

        expect(options).toContain(12);
        expect(options).toContain(1000);
        expect(options.filter((size) => size === 50)).toHaveLength(1);
        expect(
            filterTablePageSizeOptions([10, 15, 25, 50, 100], ' 5 ')
        ).toEqual([15, 25, 50]);
        expect(filterTablePageSizeOptions(options, '')).toEqual(options);
    });

    it('parses JSON responses from web requests regardless of object or text payload shape', () => {
        expect(parseWebJson({ data: { ok: true } })).toEqual({ ok: true });
        expect(parseWebJson({ data: '{"models":["gpt"]}' })).toEqual({
            models: ['gpt']
        });
        expect(parseWebJson({ data: '' })).toEqual({});
    });

    it('validates custom font stacks before they are persisted', () => {
        expect(isValidFontFamilyList('"Comic Sans MS", Arial, system-ui')).toBe(
            true
        );
        expect(isValidFontFamilyList('Noto Sans JP')).toBe(true);
        expect(isValidFontFamilyList("'Map\\'s Font', system-ui")).toBe(true);
        expect(isValidFontFamilyList('bad;font')).toBe(false);
        expect(isValidFontFamilyList('')).toBe(false);
    });

    it('quotes selected font family names for CSS stacks', () => {
        expect(quoteCssFontFamilyName('Segoe UI')).toBe("'Segoe UI'");
        expect(quoteCssFontFamilyName("'Already Quoted'")).toBe(
            "'Already Quoted'"
        );
        expect(quoteCssFontFamilyName('system-ui')).toBe('system-ui');
        expect(quoteCssFontFamilyName("Map's Font")).toBe("'Map\\'s Font'");
    });

    it('keeps only the values owned by the active custom font mode', () => {
        const draft = {
            primary: ' Segoe UI ',
            secondary: ' Noto Sans JP ',
            override: " 'Manual Font', serif "
        };

        expect(createEffectiveCustomFontDraft(draft, 'installed')).toEqual({
            primary: 'Segoe UI',
            secondary: 'Noto Sans JP',
            override: ''
        });
        expect(createEffectiveCustomFontDraft(draft, 'css')).toEqual({
            primary: '',
            secondary: '',
            override: "'Manual Font', serif"
        });
    });

    it('composes selected custom font slots into the effective stack', () => {
        expect(
            composeCustomFontFamily({
                primary: 'Segoe UI',
                secondary: 'Noto Sans JP',
                override: ''
            })
        ).toBe("'Segoe UI', 'Noto Sans JP', system-ui");
        expect(
            composeCustomFontFamily({
                primary: 'Segoe UI',
                secondary: 'segoe ui',
                override: ''
            })
        ).toBe("'Segoe UI', system-ui");
        expect(
            composeCustomFontFamily({
                primary: '',
                secondary: '',
                override: ''
            })
        ).toBe('');
    });

    it('lets an advanced custom font override replace selected slots', () => {
        expect(
            composeCustomFontFamily({
                primary: 'Segoe UI',
                secondary: 'Noto Sans JP',
                override: "'Manual Font', serif"
            })
        ).toBe("'Manual Font', serif");
    });

    it('seeds legacy custom font stacks into the advanced override', () => {
        expect(
            createCustomFontDraftFromPrefs({
                appFontFamily: 'custom',
                customFontFamily: "'Legacy Font', Arial, sans-serif",
                customFontPrimary: '',
                customFontSecondary: '',
                customFontOverride: ''
            })
        ).toEqual({
            primary: '',
            secondary: '',
            override: "'Legacy Font', Arial, sans-serif"
        });
        expect(
            createCustomFontDraftFromPrefs({
                appFontFamily: 'geist',
                customFontFamily: "'Inter Variable'",
                customFontPrimary: '',
                customFontSecondary: '',
                customFontOverride: ''
            })
        ).toEqual({
            primary: '',
            secondary: '',
            override: ''
        });
        expect(
            createCustomFontDraftFromPrefs({
                customFontFamily: "'Effective Font', system-ui",
                customFontPrimary: 'Segoe UI',
                customFontSecondary: 'Noto Sans JP',
                customFontOverride: ''
            })
        ).toEqual({
            primary: 'Segoe UI',
            secondary: 'Noto Sans JP',
            override: ''
        });
    });

    it('uses a fallback when numeric settings input is empty or invalid', () => {
        expect(parseIntegerInput('250', 100)).toBe(250);
        expect(parseIntegerInput('abc', 100)).toBe(100);
    });
});
