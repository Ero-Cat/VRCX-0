import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import type { UIMessage } from '@/domain/assistant/types';

import { AssistantMessage } from './AssistantMessage';

function assistantMessage(message: Partial<UIMessage>): UIMessage {
    return {
        id: 'asst_1',
        role: 'assistant',
        text: '',
        streaming: true,
        toolCalls: [],
        ...message
    };
}

describe('AssistantMessage', () => {
    it('renders the user message text', () => {
        const html = renderToStaticMarkup(
            <AssistantMessage
                thinkingLabel="Thinking…"
                message={{
                    id: 'user_1',
                    role: 'user',
                    text: 'Who was online last night?',
                    streaming: false,
                    toolCalls: []
                }}
            />
        );

        expect(html).toContain('Who was online last night?');
    });

    it('renders a completed assistant response as Markdown', () => {
        const html = renderToStaticMarkup(
            <AssistantMessage
                thinkingLabel="Thinking…"
                message={assistantMessage({
                    text: '**Three friends** were online.',
                    streaming: false
                })}
            />
        );

        expect(html).toContain('>Three friends</strong>');
    });

    it('shows pending tool calls as status markers before assistant text', () => {
        const html = renderToStaticMarkup(
            <AssistantMessage
                thinkingLabel="Thinking…"
                message={assistantMessage({
                    text: 'Reading local social data',
                    toolCalls: [
                        {
                            id: 'tool_1',
                            name: 'get_friend_profile',
                            args: '{}',
                            status: 'pending',
                            summary: '',
                            entities: []
                        }
                    ]
                })}
            />
        );

        expect(html).toContain('role="status"');
        expect(html).toContain('Get friend profile');
        expect(html.indexOf('Get friend profile')).toBeLessThan(
            html.indexOf('Reading local social data')
        );
    });

    it('shows the thinking marker while a whitespace-only response is streaming', () => {
        const html = renderToStaticMarkup(
            <AssistantMessage
                thinkingLabel="Thinking…"
                message={assistantMessage({ text: '\n\n' })}
            />
        );

        expect(html).toContain('role="status"');
        expect(html).toContain('Thinking…');
    });

    it('renders turn errors', () => {
        const html = renderToStaticMarkup(
            <AssistantMessage
                thinkingLabel="Thinking…"
                message={assistantMessage({
                    streaming: false,
                    error: 'The endpoint was removed.'
                })}
            />
        );

        expect(html).toContain('The endpoint was removed.');
    });
});
