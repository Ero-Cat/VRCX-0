import { describe, expect, it } from 'vitest';

import { toolDefinitionMap, toolNavDefinitions } from './tools';

describe('tool navigation definitions', () => {
    it('dispatches every pinned tool through the shared tool owner', () => {
        for (const tool of toolDefinitionMap.values()) {
            expect(
                toolNavDefinitions.find(
                    (definition) => definition.key === `tool-${tool.key}`
                )
            ).toMatchObject({
                action: { type: 'tool', toolKey: tool.key }
            });
        }
    });
});
