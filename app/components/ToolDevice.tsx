import React, { useCallback, useEffect, useState } from 'react';
import { useI18n } from '../context/I18nContext';
import { toolsPageStrings } from '../i18n/toolsPage';
import { DevicePicker, type Device } from './DevicePicker';
import { FieldLabel } from './kit/FieldLabel';

export type ToolId = 'stems' | 'midi' | 'restore' | 'recognise' | 'effects' | 'foley' | 'voice';

type ToolDeviceState = { device: Device; build: string | null; error: string | null; auto: string | null; card: string | null };

const buildName = (build: string | null | undefined) => (build === 'cpu' ? 'CPU' : build ?? undefined);

export async function readToolDevices(): Promise<Record<ToolId, ToolDeviceState>> {
  const response = await fetch('/v1/tools/devices');
  if (!response.ok) throw new Error(`/v1/tools/devices ${response.status}`);
  return (await response.json()).tools;
}

/** Where one tool computes, the same control in every tool: the choice, and the build it becomes on this computer. */
export const ToolDevice: React.FC<{
  tool: ToolId;
  disabled?: boolean;
  onChange?: () => void;
  /** Overrides whether the card is offered, for a tool that installs the card's build on its own. */
  cardAvailable?: boolean;
}> = ({ tool, disabled, onChange, cardAvailable }) => {
  const { t, language } = useI18n();
  const page = toolsPageStrings[language];
  const [state, setState] = useState<ToolDeviceState | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setState((await readToolDevices())[tool]);
      setError(null);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    }
  }, [tool]);

  useEffect(() => { void load(); }, [load]);

  const choose = async (device: Device) => {
    const response = await fetch(`/v1/tools/devices/${tool}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ device }),
    });
    if (!response.ok) {
      const body = await response.json().catch(() => null);
      setError(body?.error || `${response.status}`);
      return;
    }
    setState((await response.json()).tools[tool]);
    setError(null);
    onChange?.();
  };

  const problem = error ?? state?.error;
  const hasCard = Boolean(state?.card && state.card !== 'cpu');
  return (
    <div className="grid text-xs">
      <FieldLabel>{t('separationRuntime')}</FieldLabel>
      <fieldset disabled={disabled || !state}>
        <DevicePicker value={state?.device ?? 'auto'} onChange={device => void choose(device)}
          cardAvailable={cardAvailable ?? hasCard}
          detail={{ auto: buildName(state?.auto), cuda: hasCard ? buildName(state?.card) : undefined }} />
      </fieldset>
      {problem && <span role="alert" title={problem} className="mt-1.5 text-[13px] text-red-600 dark:text-red-400">{page.unavailable}</span>}
    </div>
  );
};
