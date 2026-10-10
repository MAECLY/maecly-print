import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import { ErrorBoundary } from './components/ErrorBoundary';
import { I18nProvider } from './i18n/I18nProvider';
import { installGlobalErrorLogging } from './lib/errorLog';
import { migrateLegacyStorage } from './lib/legacyStorage';
import { bootAppearance } from './lib/theme';
import './styles/globals.css';

// Errors no boundary or handler catches still reach the log file (NFR-033).
installGlobalErrorLogging();

// Keys saved under the former name (MaeclyPrint) move to the new prefix
// before anything reads them (ADR-0009).
migrateLegacyStorage();

// Before the first render: the theme tokens exist only under the theme class,
// and React's effects run after the first paint (see `bootAppearance`).
bootAppearance();

// The i18n provider sits OUTSIDE the boundary so the fallback panel is itself
// translated; the boundary then catches everything the app renders.
ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <I18nProvider>
      <ErrorBoundary>
        <App />
      </ErrorBoundary>
    </I18nProvider>
  </React.StrictMode>,
);
