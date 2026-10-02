// @vitest-environment jsdom

import { cleanup, fireEvent, render, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { UserDialogHeaderMedia } from './UserDialogHeaderMedia';

const iconFrame = {
    id: 'invt_frame',
    metadata: {
        assets: [
            {
                type: 'base',
                url: 'https://example.test/frame.webp'
            }
        ]
    }
};

afterEach(cleanup);

function renderMedia(frame?: typeof iconFrame) {
    return render(
        <UserDialogHeaderMedia
            bannerAlt="Profile banner"
            bannerFallbackUrl="https://example.test/legacy.webp"
            bannerUrl="https://example.test/banner.webp"
            iconFrame={frame}
            onBannerClick={vi.fn()}
            onOpenUserIcon={vi.fn()}
            userIconLabel="Open user icon"
            userIconUrl="https://example.test/icon.webp"
        />
    );
}

describe('UserDialogHeaderMedia', () => {
    it('shows only the profile banner and falls back to the avatar image when it fails', () => {
        const { container } = renderMedia(iconFrame);
        const bannerButton = within(container).getByRole('button', {
            name: 'Profile banner'
        });
        const images = [...bannerButton.querySelectorAll('img')];

        expect(images.map((image) => image.getAttribute('src'))).toEqual([
            'https://example.test/banner.webp'
        ]);

        fireEvent.error(images[0]);
        expect(
            [...bannerButton.querySelectorAll('img')].map((image) =>
                image.getAttribute('src')
            )
        ).toEqual(['https://example.test/legacy.webp']);
    });

    it('keeps the loaded banner on screen while a replacement banner loads', () => {
        const { container, rerender } = renderMedia();
        const bannerButton = within(container).getByRole('button', {
            name: 'Profile banner'
        });
        fireEvent.load(
            bannerButton.querySelector(
                'img[src="https://example.test/banner.webp"]'
            )!
        );

        rerender(
            <UserDialogHeaderMedia
                bannerAlt="Profile banner"
                bannerFallbackUrl="https://example.test/legacy.webp"
                bannerUrl="https://example.test/next.webp"
                onBannerClick={vi.fn()}
                onOpenUserIcon={vi.fn()}
                userIconLabel="Open user icon"
                userIconUrl="https://example.test/icon.webp"
            />
        );
        const images = [...bannerButton.querySelectorAll('img')];
        expect(images.map((image) => image.getAttribute('src'))).toEqual([
            'https://example.test/banner.webp',
            'https://example.test/next.webp'
        ]);
        expect(images[0].classList.contains('opacity-100')).toBe(true);
        expect(images[1].classList.contains('opacity-0')).toBe(true);

        fireEvent.load(images[1]);
        expect(
            [...bannerButton.querySelectorAll('img')].map((image) =>
                image.getAttribute('src')
            )
        ).toEqual(['https://example.test/next.webp']);

        rerender(
            <UserDialogHeaderMedia
                bannerAlt="Profile banner"
                bannerFallbackUrl="https://example.test/legacy.webp"
                bannerUrl=""
                onBannerClick={vi.fn()}
                onOpenUserIcon={vi.fn()}
                userIconLabel="Open user icon"
                userIconUrl="https://example.test/icon.webp"
            />
        );
        expect(
            [...bannerButton.querySelectorAll('img')].map((image) =>
                image.getAttribute('src')
            )
        ).toEqual(['https://example.test/legacy.webp']);
    });

    it('renders the equipped frame outside the icon button', () => {
        const { container } = renderMedia(iconFrame);

        const iconButton = within(container).getByRole('button', {
            name: 'Open user icon'
        });
        const frame = container.querySelector(
            'img[src="https://example.test/frame.webp"]'
        );

        expect(frame).not.toBeNull();
        expect(iconButton.contains(frame)).toBe(false);
    });
});
