import { createApp } from 'vue';

import App from '@/App.vue';
import '@/styles/app.css';

createApp(App).mount('#app');

if ('serviceWorker' in navigator) {
  window.addEventListener('load', () => {
    void navigator.serviceWorker.register('/service-worker.js').catch(() => {
      // PWA install remains optional; the app must still work when registration is blocked.
    });
  });
}
