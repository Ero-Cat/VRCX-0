import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import { UserStatusDot } from './UserStatusDot';

describe('UserStatusDot', () => {
    it('renders nothing without a status class', () => {
        expect(renderToStaticMarkup(<UserStatusDot />)).toBe('');
    });
});
