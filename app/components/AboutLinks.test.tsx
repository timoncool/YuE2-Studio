import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key, language: 'en' }) }));

const { AuthorLinks, AUTHOR_LINKS } = await import('./AuthorLinks');
const { SupportAuthor } = await import('./SupportAuthor');

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
});

async function show(node: React.ReactNode) {
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<>{node}</>));
}

const WALLET_ADDRESSES = ['TQST9Lp2TjK6FiVkn4fwfGUee7NmkxEE7C', '1E7dHL22RpyhJGVpcvKdbyZgksSYkYeEBC', '0xb5db65adf478983186d4897ba92fe2c25c594a0c'];

describe('About: the author and support', () => {
  it('lists the author\'s links one a line, each "Network · name" and what is there, no neuro-cartel and no other studios', async () => {
    await show(<AuthorLinks />);
    const links = [...host.querySelectorAll('li a')];
    expect(links.length).toBe(AUTHOR_LINKS.length);
    const names = links.map(link => link.querySelector('span > span:first-child')?.textContent);
    expect(names).toEqual(['Telegram · @nerual_dreming', 'X · @nerualdreming', 'Telegram · @neuroport', 'GitHub · timoncool', 'aboutSite · artgeneration.me']);
    const whats = links.map(link => link.querySelector('span > span:last-child')?.textContent);
    expect(whats).toEqual([' — aboutLinkNews', ' — aboutLinkX', ' — aboutLinkPortable', ' — aboutLinkGithub', ' — aboutLinkSite']);
    expect(host.innerHTML).not.toContain('neuro-cartel');
    expect(host.innerHTML).not.toMatch(/MiniMax|ACE-Step|studioFamily/);
    for (const icon of host.querySelectorAll('svg')) expect(icon.getAttribute('aria-hidden')).toBe('true');
  });

  it('supports the author with a card, Boosty and the donation page for the rest, no wallets in the program', async () => {
    await show(<SupportAuthor donatePage="https://timoncool.github.io/YuE2-Studio/donate.html" />);
    const buttons = [...host.querySelectorAll('a')];
    expect(buttons.map(button => button.getAttribute('href'))).toEqual(['https://dalink.to/nerual_dreming', 'https://boosty.to/neuro_art', 'https://timoncool.github.io/YuE2-Studio/donate.html']);
    expect(buttons[2].textContent).toContain('donateOther');
    for (const address of WALLET_ADDRESSES) expect(host.innerHTML).not.toContain(address);
    expect(host.querySelectorAll('code, [aria-expanded]').length).toBe(0);
    expect(host.textContent).not.toContain('allLinks');
  });
});
