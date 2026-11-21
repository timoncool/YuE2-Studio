import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { EngineSettings } from './EngineSettings';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('../services/studioQueries', () => ({ useSetupStatus: () => ({}), SELF_READ: {} }));

it.each([
  ['macos', ['auto', 'cpu'], 'computeBackendHint_macAuto'],
  ['windows', ['auto', 'cuda', 'hip', 'vulkan', 'cpu'], 'computeBackendHint_auto'],
])('uses the %s service platform for choices and Auto help', async (platform, expected, hint) => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.stubGlobal('fetch', async (url: string) => {
    if (url === '/engine/options') return Response.json({ options: { backend: 'auto' }, platform });
    if (url === '/engine/hip') return Response.json({ platform_supported: platform === 'windows', installed: true });
    return Response.json({ cards: [] });
  });
  const host = document.createElement('div');
  const root = createRoot(host);
  try {
    await act(async () => root.render(<EngineSettings />));
    const choices = [...host.querySelectorAll('[role="radiogroup"][aria-label="computeBackendLabel"] [role="radio"]')];
    expect(choices.map(button => button.textContent)).toEqual(expected.map(backend => `computeBackend_${backend}`));
    expect(host.textContent).toContain(hint);
    expect(host.textContent).not.toContain(platform === 'macos' ? 'computeBackendHint_auto' : 'computeBackendHint_macAuto');
  } finally {
    await act(async () => root.unmount());
    vi.unstubAllGlobals();
  }
});
