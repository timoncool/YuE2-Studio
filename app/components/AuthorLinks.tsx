import React from 'react';
import { Globe } from 'lucide-react';
import { SiGithub, SiTelegram, SiX } from '@icons-pack/react-simple-icons';
import { useI18n } from '../context/I18nContext';

type Network = 'Telegram' | 'X' | 'GitHub' | 'site';

/** Where to find the author, one list for every studio: "Network · name" and what is there. */
type LinkWhat = 'aboutLinkNews' | 'aboutLinkX' | 'aboutLinkPortable' | 'aboutLinkGithub' | 'aboutLinkSite';

export const AUTHOR_LINKS: { network: Network; name: string; url: string; what: LinkWhat }[] = [
  { network: 'Telegram', name: '@nerual_dreming', url: 'https://t.me/nerual_dreming', what: 'aboutLinkNews' },
  { network: 'X', name: '@nerualdreming', url: 'https://x.com/nerualdreming', what: 'aboutLinkX' },
  { network: 'Telegram', name: '@neuroport', url: 'https://t.me/neuroport', what: 'aboutLinkPortable' },
  { network: 'GitHub', name: 'timoncool', url: 'https://github.com/timoncool', what: 'aboutLinkGithub' },
  { network: 'site', name: 'artgeneration.me', url: 'https://artgeneration.me', what: 'aboutLinkSite' },
];

const ICONS: Record<Network, React.ReactNode> = {
  Telegram: <SiTelegram size={16} aria-hidden="true" className="shrink-0" />,
  X: <SiX size={16} aria-hidden="true" className="shrink-0" />,
  GitHub: <SiGithub size={16} aria-hidden="true" className="shrink-0" />,
  site: <Globe size={16} aria-hidden="true" className="shrink-0" />,
};

/** The button look of the About page: a neutral plate with a border, 44 pixels high on a phone. */
export const ABOUT_LINK = 'inline-flex items-center gap-1.5 max-sm:min-h-11 rounded-lg border border-zinc-300 px-3 py-1.5 text-xs font-medium text-zinc-700 hover:border-pink-500 hover:text-pink-500 dark:border-zinc-600 dark:text-zinc-200';

/** The heading of each block of the About page: the author, support, reporting a problem. */
export const ABOUT_HEADING = 'text-xs font-semibold uppercase tracking-wide text-zinc-900 dark:text-white';

/** The author's links as a list, one a line, each saying what is there. */
export const AuthorLinks: React.FC = () => {
  const { t } = useI18n();
  return (
    <ul className="space-y-0.5">
      {AUTHOR_LINKS.map(link => (
        <li key={link.url}>
          <a href={link.url} target="_blank" rel="noopener noreferrer" className="flex min-h-6 items-center gap-2 whitespace-nowrap rounded text-xs text-zinc-600 hover:text-pink-500 max-sm:min-h-11 dark:text-zinc-300">
            {ICONS[link.network]}
            <span>
              <span className="font-medium text-zinc-800 dark:text-zinc-100">{link.network === 'site' ? t('aboutSite') : link.network} · {link.name}</span>
              <span> — {t(link.what)}</span>
            </span>
          </a>
        </li>
      ))}
    </ul>
  );
};
