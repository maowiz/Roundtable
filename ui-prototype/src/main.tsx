import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';
import SimpleApp from './simple/SimpleApp';
import { isDesktop } from './api/client';
import './styles.css';
import './polish.css';
import './live/live.css';

// The installed app gets the simple three-screen UI; the browser keeps the design prototype.
createRoot(document.getElementById('root')!).render(<StrictMode>{isDesktop() ? <SimpleApp /> : <App />}</StrictMode>);
