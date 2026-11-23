import React, { useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { HelpCircle } from 'lucide-react';

const WIDTH = 280;
const GAP = 6;

/** The explanation's popup beside `anchor`: open while hovered or focused, closed by Esc; drawn over the window so a
 * scrolling panel does not clip it. */
function useTip(text: string) {
  const [open, setOpen] = useState(false);
  const [place, setPlace] = useState<{ left: number; top?: number; bottom?: number } | null>(null);
  const anchor = useRef<HTMLSpanElement>(null);
  const id = useId();
  useEffect(() => {
    if (!open) return;
    const measure = () => {
      const r = anchor.current?.getBoundingClientRect();
      if (!r) return;
      const left = Math.min(Math.max(8, r.left + r.width / 2 - WIDTH / 2), window.innerWidth - WIDTH - 8);
      setPlace(r.top > 140 ? { left, bottom: window.innerHeight - r.top + GAP } : { left, top: r.bottom + GAP });
    };
    measure();
    const esc = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.stopPropagation(); setOpen(false); } };
    window.addEventListener('resize', measure);
    window.addEventListener('scroll', measure, true);
    window.addEventListener('keydown', esc, true);
    return () => { window.removeEventListener('resize', measure); window.removeEventListener('scroll', measure, true); window.removeEventListener('keydown', esc, true); };
  }, [open]);
  const popup = open && place && createPortal(
    <span id={id} role="tooltip" style={{ left: place.left, top: place.top, bottom: place.bottom, width: WIDTH }}
      className="fixed z-[90] pointer-events-none rounded-lg border border-[var(--color-border)] bg-[var(--color-overlay)] px-2.5 py-1.5 text-[12px] leading-snug text-[var(--color-text)] normal-case tracking-normal font-normal shadow-xl anim-pop">
      {text}
    </span>,
    document.body,
  );
  const events = { onMouseEnter: () => setOpen(true), onMouseLeave: () => setOpen(false), onFocus: () => setOpen(true), onBlur: () => setOpen(false) };
  return { open, setOpen, anchor, id, popup, events };
}

/** A faint «?» beside a label, a toggle or a list, which have no hover of their own: hover, focus or a tap opens what
 * the control does and how it works. */
export function Hint({ text }: { text: string }) {
  const { open, setOpen, anchor, id, popup, events } = useTip(text);
  return (
    <>
      <span ref={anchor} tabIndex={0} role="button" aria-label={text} aria-describedby={open ? id : undefined} {...events}
        onClick={(e) => { e.preventDefault(); e.stopPropagation(); setOpen((v) => !v); }}
        className={`relative cursor-help inline-flex align-middle rounded-full transition after:absolute after:-inset-1.5 after:content-[''] hover:opacity-100 hover:text-[var(--color-accent-2)] focus-visible:opacity-100 focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--color-accent)] ${open ? 'opacity-100 text-[var(--color-accent-2)]' : 'opacity-40 text-[var(--color-muted)]'}`}>
        <HelpCircle size={12} />
      </span>
      {popup}
    </>
  );
}

/** The same explanation on a button itself, without a «?»: it opens while the button is hovered or focused. */
export function Tip({ text, children, className = 'inline-flex' }: { text: string; children: ReactNode; className?: string }) {
  const { open, anchor, id, popup, events } = useTip(text);
  return (
    <span ref={anchor} aria-describedby={open ? id : undefined} {...events} className={className}>
      {children}
      {popup}
    </span>
  );
}
