import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';

vi.mock('@/services/entityMediaService', () => ({
    openExternalLink: vi.fn()
}));

import { ChangelogMarkdown } from './ChangelogDialog';

describe('ChangelogMarkdown', () => {
    it('renders GitHub-style soft line breaks in release markdown', () => {
        const html = renderToStaticMarkup(
            React.createElement(
                ChangelogMarkdown,
                null,
                'v2.24.3\n**新增内容：**'
            )
        );

        expect(html).toMatch(
            /v2\.24\.3<br\s*\/?>\s*<strong>新增内容：<\/strong>/
        );
    });
});
