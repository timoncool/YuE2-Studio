import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, expect, it, vi } from 'vitest';

vi.mock('../../context/I18nContext', () => ({ useI18n: () => ({ language: 'en', t: (key: string) => key }) }));
vi.mock('../../services/studioQueries', () => ({ useSetupStatus: () => ({ data: undefined }) }));
vi.mock('../ProblemReportDialog', () => ({ ProblemReportDialog: () => <div data-testid="report" /> }));

const { default: Combobox } = await import('./Combobox');
const { Hint } = await import('./Hint');
const { ErrorNote } = await import('./ErrorNote');

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  vi.unstubAllGlobals();
});

function mount(node: React.ReactNode) {
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  act(() => root.render(node));
}

const type = (input: HTMLInputElement, value: string) => act(() => {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, value);
  input.dispatchEvent(new Event('input', { bubbles: true }));
});
const key = (target: Element, name: string) => act(() => { target.dispatchEvent(new KeyboardEvent('keydown', { key: name, bubbles: true })); });

it('narrows the list by a piece of a name and picks with the keyboard', () => {
  const onChange = vi.fn();
  mount(<Combobox label="Model" value="ace" onChange={onChange} options={[
    { value: 'yue2', label: 'YuE2' }, { value: 'ace', label: 'ACE-Step' }, { value: 'mm', label: 'MiniMax Music3' },
  ]} />);
  const input = host.querySelector('input')!;
  expect(input.value).toBe('ACE-Step');
  act(() => input.focus());
  type(input, 'mini');
  expect([...document.querySelectorAll('[role=option]')].map(option => option.textContent)).toEqual(['MiniMax Music3']);
  key(input, 'Enter');
  expect(onChange).toHaveBeenCalledWith('mm');
  expect(document.querySelector('[role=listbox]')).toBeNull();
});

it('opens the explanation on focus and closes it with Escape', () => {
  mount(<Hint text="What the control does" />);
  const anchor = host.querySelector('[role=button]') as HTMLElement;
  act(() => anchor.focus());
  expect(document.querySelector('[role=tooltip]')?.textContent).toBe('What the control does');
  key(window.document.body, 'Escape');
  expect(document.querySelector('[role=tooltip]')).toBeNull();
});

it('keeps the error with copy, diagnostics and a report', async () => {
  const writeText = vi.fn(async () => undefined);
  vi.stubGlobal('navigator', { clipboard: { writeText } });
  const opened = vi.fn();
  window.addEventListener('yue:open-settings', opened);
  mount(<ErrorNote text="the local engine runtime was not found" hint="Reinstall the studio." />);
  const buttons = [...host.querySelectorAll('button')];
  expect(host.textContent).toContain('Reinstall the studio.');
  await act(async () => buttons.find(button => button.textContent === 'Copy')!.click());
  expect(writeText).toHaveBeenCalledWith('the local engine runtime was not found');
  act(() => buttons.find(button => button.textContent === 'Diagnostics')!.click());
  expect((opened.mock.calls[0][0] as CustomEvent).detail).toBe('engine');
  act(() => buttons.find(button => button.textContent === 'problemTitle')!.click());
  expect(host.querySelector('[data-testid=report]')).not.toBeNull();
  window.removeEventListener('yue:open-settings', opened);
});
