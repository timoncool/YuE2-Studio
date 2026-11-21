/** Shared DJ-mode visibility for both the player bar and the collapsed/expanded sidebar. */
let active = false;
const listeners = new Set<(value: boolean) => void>();

export const djModeOn = () => active;

export function setDjModeOn(value: boolean): void {
  if (active === value) return;
  active = value;
  listeners.forEach(listener => listener(active));
}

export function onDjMode(listener: (value: boolean) => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}
