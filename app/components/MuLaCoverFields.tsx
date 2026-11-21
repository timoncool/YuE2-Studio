import React, { useState } from 'react';
import { useI18n } from '../context/I18nContext';
import { useLibrarySongs } from '../services/studioQueries';

const strings = {
  en: { original: 'Original song', choose: 'Choose from the library', install: 'Install resampler', installing: 'Installing resampler…', error: 'Could not install the MuLaCover resampler.', license: 'Noncommercial weights and output terms' },
  ru: { original: 'Исходная песня', choose: 'Выберите из библиотеки', install: 'Установить ресемплер', installing: 'Устанавливается ресемплер…', error: 'Не удалось установить ресемплер MuLaCover.', license: 'Условия некоммерческого использования весов и результатов' },
  zh: { original: '原曲', choose: '从资料库选择', install: '安装重采样器', installing: '正在安装重采样器…', error: '无法安装 MuLaCover 重采样器。', license: '权重和输出的非商业使用条款' },
  ja: { original: '原曲', choose: 'ライブラリから選択', install: 'リサンプラーをインストール', installing: 'リサンプラーをインストール中…', error: 'MuLaCover のリサンプラーをインストールできませんでした。', license: '重みと出力の非商用利用条件' },
  ko: { original: '원곡', choose: '라이브러리에서 선택', install: '리샘플러 설치', installing: '리샘플러 설치 중…', error: 'MuLaCover 리샘플러를 설치하지 못했습니다.', license: '가중치와 출력의 비상업적 사용 조건' },
  pt: { original: 'Canção original', choose: 'Escolha na biblioteca', install: 'Instalar reamostrador', installing: 'Instalando reamostrador…', error: 'Não foi possível instalar o reamostrador do MuLaCover.', license: 'Termos de uso não comercial dos pesos e das saídas' },
};

type Props = {
  value: string;
  onChange: (id: string) => void;
  onInstall: () => void;
  missingSoxr: boolean;
  controlClassName: string;
};

export function MuLaCoverFields({ value, onChange, onInstall, missingSoxr, controlClassName }: Props) {
  const { language } = useI18n();
  const text = strings[language];
  const { data } = useLibrarySongs();
  const songs = (data ?? []).filter(song => song.audioUrl && !song.isGenerating);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState('');

  const install = async () => {
    setError('');
    setInstalling(true);
    try {
      const response = await fetch('/v1/music/audio-cpp/mulacover/runtime', { method: 'POST' });
      if (!response.ok) {
        const body = await response.json();
        throw new Error(typeof body.error === 'string' ? body.error : text.error);
      }
      onInstall();
    } catch (error) {
      setError(error instanceof Error ? error.message : text.error);
    } finally {
      setInstalling(false);
    }
  };

  return (
    <div className="space-y-2">
      <label className="grid gap-1">
        <span>{text.original}</span>
        <select required className={controlClassName} value={value}
          onChange={event => onChange(event.target.value)}>
          <option value="">{text.choose}</option>
          {value && !songs.some(song => song.id === value) && <option value={value} disabled>{value}</option>}
          {songs.map(song => <option key={song.id} value={song.id}>{song.title}</option>)}
        </select>
      </label>
      <a className="text-xs underline" target="_blank" rel="noreferrer"
        href="https://github.com/HeartMuLa/MuLaCover/blob/e27fa7fa0da7c9817879cc6210cc5693f9327946/MODEL_LICENSE">{text.license}</a>
      {missingSoxr && <div>
        <button type="button" className="text-xs underline disabled:opacity-50" disabled={installing}
          onClick={() => void install()}>{installing ? text.installing : text.install}</button>
        {error && <p role="alert" className="text-xs">{error}</p>}
      </div>}
    </div>
  );
}
