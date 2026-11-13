import React, { useState } from 'react';
import { Check, ChevronDown, ChevronUp, Copy, CreditCard, ExternalLink } from 'lucide-react';
import { SiBoosty } from '@icons-pack/react-simple-icons';
import { useI18n } from '../context/I18nContext';
import { ABOUT_LINK } from './AuthorLinks';

/** The author's wallets, as the donation page lists them, in its order. */
export const WALLETS: { coin: string; network: string; address: string; note?: 'donateUsdtNote' }[] = [
  { coin: 'USDT', network: 'TRON (TRC20)', address: 'TQST9Lp2TjK6FiVkn4fwfGUee7NmkxEE7C', note: 'donateUsdtNote' },
  { coin: 'BTC', network: 'Bitcoin', address: '1E7dHL22RpyhJGVpcvKdbyZgksSYkYeEBC' },
  { coin: 'ETH', network: 'Ethereum (ERC20)', address: '0xb5db65adf478983186d4897ba92fe2c25c594a0c' },
];

const WALLETS_ID = 'about-wallets';

const Wallet: React.FC<{ wallet: (typeof WALLETS)[number] }> = ({ wallet }) => {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  const copy = () => {
    void navigator.clipboard.writeText(wallet.address).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    });
  };
  const label = copied ? t('donateCopied') : t('donateCopyAddress').replace('{coin}', wallet.coin);
  return (
    <li className="space-y-1">
      <div className="flex items-center gap-2">
        <span className="w-36 shrink-0 text-xs text-zinc-500 dark:text-zinc-400">{wallet.coin} · {wallet.network}</span>
        <code className="min-w-0 flex-1 select-all break-all rounded-md bg-zinc-100 px-2 py-1 font-mono text-[11px] text-zinc-700 sm:whitespace-nowrap sm:break-normal dark:bg-black/30 dark:text-zinc-300">{wallet.address}</code>
        <button
          type="button"
          onClick={copy}
          aria-label={label}
          title={label}
          className="inline-flex h-[30px] w-[30px] max-sm:min-h-11 max-sm:min-w-11 shrink-0 items-center justify-center rounded-lg border border-zinc-300 text-zinc-600 hover:border-pink-500 hover:text-pink-500 dark:border-zinc-600 dark:text-zinc-300"
        >
          {copied ? <Check size={14} aria-hidden="true" className="text-emerald-500" /> : <Copy size={14} aria-hidden="true" />}
        </button>
      </div>
      {wallet.note && <p className="text-[11px] text-zinc-500 dark:text-zinc-400">{t(wallet.note)}</p>}
    </li>
  );
};

/** Support the author, as the donation page of the studio's site has it: a card, a monthly subscription, crypto. */
export const SupportAuthor: React.FC<{ donatePage: string }> = ({ donatePage }) => {
  const { t } = useI18n();
  const [crypto, setCrypto] = useState(false);
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap gap-2">
        <a href="https://dalink.to/nerual_dreming" target="_blank" rel="noopener noreferrer" className={ABOUT_LINK}><CreditCard size={14} aria-hidden="true" />{t('donateCardButton')}</a>
        <a href="https://boosty.to/neuro_art" target="_blank" rel="noopener noreferrer" className={ABOUT_LINK}><SiBoosty size={14} aria-hidden="true" />{t('donateMonthlyButton')}</a>
        <button type="button" aria-expanded={crypto} aria-controls={WALLETS_ID} onClick={() => setCrypto(open => !open)} className={ABOUT_LINK}>
          {t('donateCrypto')}
          {crypto ? <ChevronUp size={14} aria-hidden="true" /> : <ChevronDown size={14} aria-hidden="true" />}
        </button>
      </div>
      <ul id={WALLETS_ID} hidden={!crypto} className="space-y-2">
        {crypto && WALLETS.map(wallet => <Wallet key={wallet.coin} wallet={wallet} />)}
      </ul>
      <a href={donatePage} target="_blank" rel="noopener noreferrer" className="inline-flex min-h-6 items-center gap-1 text-xs text-zinc-500 underline underline-offset-2 hover:text-pink-500 dark:text-zinc-400"><ExternalLink size={12} aria-hidden="true" />{t('donateAllWays')}</a>
    </div>
  );
};
