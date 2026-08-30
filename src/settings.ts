import { mount } from 'svelte'
import './lib/styles/tokens.css'
import './lib/styles/global.css'
import Settings from './routes/Settings.svelte'

export default mount(Settings, { target: document.getElementById('app')! })
