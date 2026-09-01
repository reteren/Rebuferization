import { mount } from 'svelte'
import './lib/styles/tokens.css'
import './lib/styles/global.css'
import TrayMenu from './routes/TrayMenu.svelte'

export default mount(TrayMenu, { target: document.getElementById('app')! })
