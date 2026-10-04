import './styles.css';

import { initI18n } from '@scrin/i18n';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './app';
import { host } from './host';

initI18n({ storage: host.storage });

const el = document.getElementById('root');
if (!el) throw new Error('#root missing from index.html');

createRoot(el).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
