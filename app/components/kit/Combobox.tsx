import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";

export type ComboOption = { value: string; label: string; search?: string; group?: string; disabled?: boolean; hint?: string };

/** A list with a filter field (Dub Studio's Combobox): typing a piece of a name narrows it; arrows and Enter pick, Escape
 * closes. Options keep their order; a change of `group` between neighbours starts a labelled group. */
export default function Combobox({ value, options, onChange, label, placeholder, noResults, className, inputClassName }: {
  value: string;
  options: ComboOption[];
  onChange: (value: string) => void;
  label: string;
  placeholder?: string;
  noResults?: string;
  className?: string;
  inputClassName?: string;
}) {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const [active, setActive] = useState(0);
  const [place, setPlace] = useState<{ left: number; width: number; top?: number; bottom?: number; height: number } | null>(null);
  const box = useRef<HTMLDivElement>(null);
  const list = useId();
  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => {
      const at = e.target as Node;
      if (box.current && !box.current.contains(at) && !listBox.current?.contains(at)) close();
    };
    document.addEventListener("mousedown", away);
    return () => document.removeEventListener("mousedown", away);
  }, [open]);
  const listBox = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const measure = () => {
      const r = box.current?.getBoundingClientRect();
      if (!r) return;
      const below = window.innerHeight - r.bottom - 8, above = r.top - 8;
      const height = Math.min(288, below >= Math.min(160, above) ? below : above);
      setPlace(below >= Math.min(160, above) ? { left: r.left, width: r.width, top: r.bottom + 4, height } : { left: r.left, width: r.width, bottom: window.innerHeight - r.top + 4, height });
    };
    measure();
    window.addEventListener("resize", measure);
    window.addEventListener("scroll", measure, true);
    return () => { window.removeEventListener("resize", measure); window.removeEventListener("scroll", measure, true); };
  }, [open]);
  useEffect(() => {
    const pane = listBox.current;
    const item = open ? document.getElementById(`${list}-${active}`) : null;
    if (!pane || !item) return;
    if (item.offsetTop < pane.scrollTop) pane.scrollTop = item.offsetTop;
    else if (item.offsetTop + item.offsetHeight > pane.scrollTop + pane.clientHeight) pane.scrollTop = item.offsetTop + item.offsetHeight - pane.clientHeight;
  }, [open, active, list, place]);
  const ql = q.trim().toLowerCase();
  const shown = ql ? options.filter((o) => `${o.label} ${o.value} ${o.search ?? ""} ${o.hint ?? ""}`.toLowerCase().includes(ql)) : options;
  const pickable = shown.map((o, i) => (o.disabled ? -1 : i)).filter((i) => i >= 0);
  const current = options.find((o) => o.value === value)?.label ?? value;
  function close() { setOpen(false); setQ(""); }
  const commit = (v: string) => { onChange(v); close(); };
  const step = (by: number) => {
    if (!pickable.length) return;
    const at = pickable.indexOf(active);
    setActive(pickable[(at + by + pickable.length) % pickable.length]);
  };
  return (
    <div ref={box} className={`relative ${className ?? ""}`}>
      <input
        role="combobox" aria-label={label} aria-expanded={open} aria-controls={list} aria-autocomplete="list"
        aria-activedescendant={open && shown[active] ? `${list}-${active}` : undefined}
        value={open ? q : current} placeholder={open ? current || placeholder : placeholder} title={current}
        onFocus={() => { setOpen(true); setQ(""); setActive(Math.max(0, shown.findIndex((o) => o.value === value))); }}
        onClick={() => { if (!open) { setOpen(true); setQ(""); setActive(Math.max(0, shown.findIndex((o) => o.value === value))); } }}
        onChange={(e) => { setQ(e.target.value); setOpen(true); setActive(0); }}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") { e.preventDefault(); setOpen(true); step(1); }
          else if (e.key === "ArrowUp") { e.preventDefault(); setOpen(true); step(-1); }
          else if (e.key === "Enter" && open) {
            e.preventDefault();
            const pick = shown[active] && !shown[active].disabled ? shown[active] : shown[pickable[0]];
            if (pick) { commit(pick.value); (e.target as HTMLInputElement).blur(); }
          } else if (e.key === "Escape" && open) { e.stopPropagation(); close(); (e.target as HTMLInputElement).blur(); }
        }}
        className={inputClassName ?? "w-full text-[var(--color-text)] bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-lg px-2 py-1.5 text-[12px] focus:border-[var(--color-accent)] focus:outline-none"} />
      {open && place && createPortal(
        <div ref={listBox} id={list} role="listbox" aria-label={label}
          style={{ left: place.left, top: place.top, bottom: place.bottom, width: Math.max(place.width, 200), maxHeight: place.height }}
          className="fixed z-[80] overflow-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] shadow-xl">
          {shown.length === 0 && <div className="px-2.5 py-2 text-[12px] text-[var(--color-muted)]">{noResults ?? "∅"}</div>}
          {shown.map((o, i) => (
            <div key={`${o.group ?? ""}:${o.value}`}>
              {o.group && o.group !== shown[i - 1]?.group && (
                <div className="px-2.5 pt-2 pb-1 text-[10px] uppercase tracking-[0.12em] text-[var(--color-muted)]">{o.group}</div>
              )}
              <div id={`${list}-${i}`} role="option" aria-selected={o.value === value} aria-disabled={o.disabled || undefined}
                onMouseDown={(e) => e.preventDefault()} onMouseEnter={() => !o.disabled && setActive(i)}
                onClick={() => !o.disabled && commit(o.value)} title={o.hint ? `${o.label}\n${o.hint}` : o.label}
                className={`px-2.5 py-1.5 text-[12px] ${o.hint ? "" : "truncate"} ${o.disabled ? "text-[var(--color-muted)] cursor-default" : "text-[var(--color-text)] cursor-pointer"} ${i === active && !o.disabled ? "bg-[var(--color-overlay)]" : ""} ${o.value === value ? "!text-[var(--color-accent)]" : ""}`}>
                {o.hint ? <><div className="truncate">{o.label}</div><div className="text-[11px] leading-snug text-[var(--color-muted)]">{o.hint}</div></> : o.label}
              </div>
            </div>
          ))}
        </div>,
        document.body,
      )}
    </div>
  );
}
