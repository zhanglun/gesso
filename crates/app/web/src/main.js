import { mount } from 'svelte';
import App from './App.svelte';
import './styles.css';

const target = document.getElementById('app');
if (!target) throw new Error('missing_app_mount');
mount(App, { target });
