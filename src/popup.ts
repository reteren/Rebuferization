import { mount } from 'svelte'
import './lib/styles/tokens.css'
import './lib/styles/global.css'
import Popup from './routes/Popup.svelte'

export default mount(Popup, { target: document.getElementById('app')! })
